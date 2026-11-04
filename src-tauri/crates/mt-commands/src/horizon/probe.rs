//! Sondagem estática e dinâmica via PHP embarcado do Laravel Horizon.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use mt_core::{MtError, MtResult, ProjectId};
use mt_db::sanitizer::sanitize;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::process::Command;

use crate::artisan::runner::run_bounded;
use crate::fs_util::read_capped;
use crate::horizon::model::{
    HorizonAvailability, HorizonContext, HorizonSnapshot, HorizonStatus, QueueScope,
};
use crate::ports::RunLimits;

const PROBE_TIMEOUT_SECS: u64 = 5;
const MAX_PROBE_STDOUT_BYTES: usize = 256 * 1024;
const MAX_PROBE_STDERR_BYTES: usize = 32 * 1024;

/// Saída do helper PHP embarcado para sondagem do Horizon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawHorizonProbeOutput {
    pub ok: bool,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub horizon_version: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub scope_fingerprint: Option<String>,
    #[serde(default)]
    pub masters_count: usize,
    #[serde(default)]
    pub queues: Vec<RawQueueScope>,
    #[serde(default)]
    pub pending: Option<u64>,
    #[serde(default)]
    pub running: Option<u64>,
    #[serde(default)]
    pub failed: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Par conexão-fila na saída raw.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawQueueScope {
    pub connection: String,
    pub queue: String,
}

/// Script PHP constante e embarcado para inspeção estrita do Laravel Horizon.
pub const EMBEDDED_HORIZON_PROBE_PHP: &str = r#"
declare(strict_types=1);

error_reporting(E_ALL & ~E_DEPRECATED & ~E_USER_DEPRECATED & ~E_NOTICE);
ini_set('display_errors', '0');

$response = [
    'ok' => false,
    'installed' => false,
    'horizon_version' => null,
    'environment' => null,
    'scope_fingerprint' => null,
    'masters_count' => 0,
    'queues' => [],
    'pending' => null,
    'running' => null,
    'failed' => null,
    'error' => null,
];

try {
    $autoload = getcwd() . '/vendor/autoload.php';
    if (!file_exists($autoload)) {
        $response['error'] = 'Autoload do Composer não encontrado em vendor/autoload.php';
        echo json_encode($response);
        exit(0);
    }
    require_once $autoload;

    $bootstrap = getcwd() . '/bootstrap/app.php';
    if (!file_exists($bootstrap)) {
        $response['error'] = 'Arquivo bootstrap/app.php do Laravel não encontrado';
        echo json_encode($response);
        exit(0);
    }
    $app = require_once $bootstrap;

    if ($app instanceof \Illuminate\Contracts\Foundation\Application) {
        $app->make(\Illuminate\Contracts\Console\Kernel::class)->bootstrap();
    } elseif (method_exists($app, 'make')) {
        try {
            $kernel = $app->make(\Illuminate\Contracts\Console\Kernel::class);
            if (method_exists($kernel, 'bootstrap')) {
                $kernel->bootstrap();
            }
        } catch (\Throwable $e) {}
    }

    if (!class_exists(\Laravel\Horizon\Horizon::class)) {
        $response['ok'] = true;
        $response['installed'] = false;
        echo json_encode($response);
        exit(0);
    }

    $response['installed'] = true;

    // Versão do Horizon
    if (defined('\Laravel\Horizon\Horizon::VERSION')) {
        $response['horizon_version'] = \Laravel\Horizon\Horizon::VERSION;
    } elseif (class_exists(\Composer\InstalledVersions::class)) {
        try {
            $response['horizon_version'] = \Composer\InstalledVersions::getVersion('laravel/horizon');
        } catch (\Throwable $e) {}
    }

    $env = app()->environment();
    $response['environment'] = is_string($env) ? $env : 'unknown';

    // Resolução de pares únicos (connection, queue)
    $queuePairs = [];
    $envSupervisors = config('horizon.environments.' . $response['environment'], config('horizon.defaults', []));
    if (is_array($envSupervisors)) {
        foreach ($envSupervisors as $supervisorName => $supConfig) {
            if (!is_array($supConfig)) continue;
            $conn = $supConfig['connection'] ?? 'redis';
            $queues = $supConfig['queue'] ?? ['default'];
            if (is_string($queues)) {
                $queues = array_map('trim', explode(',', $queues));
            }
            if (is_array($queues)) {
                foreach ($queues as $q) {
                    if (is_string($q) && trim($q) !== '') {
                        $key = $conn . '|' . trim($q);
                        $queuePairs[$key] = [
                            'connection' => $conn,
                            'queue' => trim($q),
                        ];
                    }
                }
            }
        }
    }

    if (empty($queuePairs)) {
        $defConn = (string) config('queue.default', 'redis');
        $defQueue = (string) config("queue.connections.{$defConn}.queue", 'default');
        $queuePairs[$defConn . '|' . $defQueue] = [
            'connection' => $defConn,
            'queue' => $defQueue,
        ];
    }

    $response['queues'] = array_values($queuePairs);

    // Impressão digital opaca de destino
    $prefix = (string) config('horizon.prefix', 'horizon:');
    $destStr = $response['environment'] . ':' . $prefix . ':' . implode(';', array_keys($queuePairs));
    $response['scope_fingerprint'] = hash('sha256', $destStr);

    // Detecção de masters ativos
    $activeMasters = 0;
    try {
        if (interface_exists(\Laravel\Horizon\Contracts\MasterSupervisorRepository::class)) {
            $masterRepo = app(\Laravel\Horizon\Contracts\MasterSupervisorRepository::class);
            if (method_exists($masterRepo, 'all')) {
                $masters = $masterRepo->all();
                if (is_array($masters)) {
                    foreach ($masters as $m) {
                        $status = is_object($m) && isset($m->status) ? (string) $m->status : '';
                        if ($status !== 'paused' && $status !== '') {
                            $activeMasters++;
                        }
                    }
                }
            }
        }
    } catch (\Throwable $e) {}
    $response['masters_count'] = $activeMasters;

    // Métricas de filas: ready + delayed (pendentes) e reserved (executando)
    $totalReady = 0;
    $totalDelayed = 0;
    $totalReserved = 0;
    $metricsError = null;

    try {
        $queueManager = app('queue');
        foreach ($response['queues'] as $qItem) {
            $connName = $qItem['connection'];
            $queueName = $qItem['queue'];

            try {
                $queueConn = $queueManager->connection($connName);
                if (method_exists($queueConn, 'getRedis')) {
                    $redis = $queueConn->getRedis();
                    
                    // Ready jobs na lista
                    $readyCount = 0;
                    try {
                        $readyCount = (int) $redis->llen('queues:' . $queueName);
                    } catch (\Throwable $e) {
                        try {
                            $readyCount = (int) $redis->llen($queueName);
                        } catch (\Throwable $e2) {}
                    }
                    $totalReady += max(0, $readyCount);

                    // Delayed jobs no sorted set
                    $delayedCount = 0;
                    try {
                        $delayedCount = (int) $redis->zcard('queues:' . $queueName . ':delayed');
                    } catch (\Throwable $e) {}
                    $totalDelayed += max(0, $delayedCount);

                    // Reserved jobs no sorted set
                    $reservedCount = 0;
                    try {
                        $reservedCount = (int) $redis->zcard('queues:' . $queueName . ':reserved');
                    } catch (\Throwable $e) {}
                    $totalReserved += max(0, $reservedCount);
                }
            } catch (\Throwable $qe) {
                $metricsError = $qe->getMessage();
            }
        }

        if ($metricsError === null) {
            $response['pending'] = $totalReady + $totalDelayed;
            $response['running'] = $totalReserved;
        } else {
            $response['error'] = 'Falha ao acessar métricas do Redis: ' . $metricsError;
        }
    } catch (\Throwable $e) {
        $response['error'] = 'Falha no subsistema de filas: ' . $e->getMessage();
    }

    // Contagem de falhas via Horizon JobRepository
    try {
        if (interface_exists(\Laravel\Horizon\Contracts\JobRepository::class)) {
            $jobRepo = app(\Laravel\Horizon\Contracts\JobRepository::class);
            if (method_exists($jobRepo, 'countFailed')) {
                $failed = $jobRepo->countFailed();
                if ($failed !== null && is_numeric($failed)) {
                    $response['failed'] = (int) $failed;
                }
            }
        }
    } catch (\Throwable $e) {}

    $response['ok'] = true;
} catch (\Throwable $e) {
    $response['error'] = $e->getMessage();
}

echo json_encode($response);
"#;

/// Verifica se a pasta do projeto possui indícios estáticos do pacote `laravel/horizon`.
#[must_use]
pub fn has_static_horizon_manifest(root: &Path) -> bool {
    let composer_json = root.join("composer.json");
    if let Ok(Some(content)) = read_capped(&composer_json, 1024 * 1024) {
        if content.contains("laravel/horizon") {
            return true;
        }
    }

    let composer_lock = root.join("composer.lock");
    if let Ok(Some(content)) = read_capped(&composer_lock, 2 * 1024 * 1024) {
        if content.contains("laravel/horizon") {
            return true;
        }
    }

    false
}

/// Executa o probe PHP para inspecionar dinamicamente o status e métricas do Horizon.
pub async fn run_horizon_probe(root: &Path) -> MtResult<RawHorizonProbeOutput> {
    let mut cmd = Command::new("php");
    cmd.args(["-r", EMBEDDED_HORIZON_PROBE_PHP])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);

    let limits = RunLimits {
        timeout: Duration::from_secs(PROBE_TIMEOUT_SECS),
        max_stdout: MAX_PROBE_STDOUT_BYTES,
        max_stderr: MAX_PROBE_STDERR_BYTES,
    };

    let raw_bytes = run_bounded(cmd, limits).await?;
    let raw_text = String::from_utf8_lossy(&raw_bytes);

    let parsed: RawHorizonProbeOutput = serde_json::from_str(raw_text.trim()).map_err(|e| {
        MtError::new(
            mt_core::ErrorCode("horizon.probe_invalid_json"),
            format!(
                "Resposta inválida do probe PHP do Horizon: {e}. Saída bruta: {}",
                sanitize(&raw_text)
            ),
        )
    })?;

    Ok(parsed)
}

/// Compõe o [`HorizonSnapshot`] a partir do probe e do estado de supervisão do runtime.
#[must_use]
pub fn build_snapshot_from_probe(
    project_id: ProjectId,
    root: &Path,
    probe: Option<&RawHorizonProbeOutput>,
    supervised_status: Option<HorizonStatus>,
    session: Option<crate::horizon::model::HorizonSessionDto>,
    revision: u64,
    sampled_at_ms: i64,
) -> HorizonSnapshot {
    let root_str = root.display().to_string();

    let (
        availability,
        horizon_version,
        environment,
        queues,
        scope_id,
        pending,
        running,
        failed,
        diag,
        has_external_master,
    ) = match probe {
        Some(p) if !p.installed => (
            HorizonAvailability::Unavailable,
            None,
            None,
            Vec::new(),
            String::new(),
            None,
            None,
            None,
            p.error.as_deref().map(|s| sanitize(s).into_owned()),
            false,
        ),
        Some(p) => {
            let queues = p
                .queues
                .iter()
                .map(|q| QueueScope {
                    connection: q.connection.clone(),
                    queue: q.queue.clone(),
                })
                .collect::<Vec<_>>();

            let scope = p.scope_fingerprint.clone().unwrap_or_else(|| {
                let mut hasher = Sha256::new();
                hasher.update(root_str.as_bytes());
                format!("{:x}", hasher.finalize())
            });

            let avail = if p.error.is_some() && p.pending.is_none() {
                HorizonAvailability::Error
            } else {
                HorizonAvailability::Available
            };

            (
                avail,
                p.horizon_version.clone(),
                p.environment.clone(),
                queues,
                scope,
                p.pending,
                p.running,
                p.failed,
                p.error.as_deref().map(|s| sanitize(s).into_owned()),
                p.masters_count > 0,
            )
        }
        None => (
            HorizonAvailability::Unavailable,
            None,
            None,
            Vec::new(),
            String::new(),
            None,
            None,
            None,
            None,
            false,
        ),
    };

    // Determina o status real do runtime
    let status = if let Some(sup) = supervised_status {
        sup
    } else if has_external_master {
        HorizonStatus::External
    } else if availability == HorizonAvailability::Error {
        HorizonStatus::Error
    } else {
        HorizonStatus::Stopped
    };

    let can_start = availability == HorizonAvailability::Available
        && matches!(
            status,
            HorizonStatus::Stopped | HorizonStatus::Error | HorizonStatus::External
        );

    let can_stop = matches!(status, HorizonStatus::Running | HorizonStatus::Starting);

    let can_clear_pending = availability == HorizonAvailability::Available
        && !queues.is_empty()
        && pending.unwrap_or(0) > 0;

    let context_revision = {
        let mut hasher = Sha256::new();
        hasher.update(root_str.as_bytes());
        hasher.update(environment.as_deref().unwrap_or("").as_bytes());
        hasher.update(scope_id.as_bytes());
        for q in &queues {
            hasher.update(q.connection.as_bytes());
            hasher.update(q.queue.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    };

    HorizonSnapshot {
        context: HorizonContext {
            project_id,
            root: root_str,
            context_revision,
            scope_id,
        },
        availability,
        horizon_version,
        queues,
        environment,
        status,
        pending,
        running,
        failed,
        sampled_at_ms,
        revision,
        diagnostic: diag,
        can_start,
        can_stop,
        can_clear_pending,
        session,
    }
}
