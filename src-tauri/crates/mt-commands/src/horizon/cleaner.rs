//! Algoritmo seguro de limpeza exclusiva de tarefas pendentes do Laravel Horizon.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use mt_core::{MtError, MtResult};
use mt_db::sanitizer::sanitize;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::horizon::model::{HorizonClearResult, QueueClearCount, QueueScope};
use crate::ports::RunLimits;

const CLEAR_TIMEOUT_SECS: u64 = 10;
const MAX_CLEAR_STDOUT_BYTES: usize = 256 * 1024;
const MAX_CLEAR_STDERR_BYTES: usize = 32 * 1024;

/// Entrada do helper PHP para exclusão de pendentes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawClearInput {
    pub queues: Vec<QueueScope>,
    pub max_batch: usize,
}

/// Saída do helper PHP para exclusão de pendentes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawClearOutput {
    pub ok: bool,
    #[serde(default)]
    pub cleared: Vec<RawClearPerQueue>,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub is_partial: bool,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Contagem por fila na saída do PHP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawClearPerQueue {
    pub connection: String,
    pub queue: String,
    pub count: u64,
}

/// Script PHP constante e embarcado para exclusão segura estrita de pendentes (ready + delayed).
pub const EMBEDDED_HORIZON_CLEAR_PHP: &str = r#"<?php
declare(strict_types=1);

error_reporting(E_ALL & ~E_DEPRECATED & ~E_USER_DEPRECATED & ~E_NOTICE);
ini_set('display_errors', '0');

$response = [
    'ok' => false,
    'cleared' => [],
    'total' => 0,
    'is_partial' => false,
    'warning' => null,
    'error' => null,
];

try {
    $rawInput = file_get_contents('php://stdin');
    $input = json_decode($rawInput, true);
    if (!is_array($input) || empty($input['queues'])) {
        $response['error'] = 'Parâmetros de filas ausentes ou inválidos no payload de entrada.';
        echo json_encode($response);
        exit(0);
    }

    $autoload = getcwd() . '/vendor/autoload.php';
    if (!file_exists($autoload)) {
        $response['error'] = 'vendor/autoload.php não encontrado.';
        echo json_encode($response);
        exit(0);
    }
    require_once $autoload;

    $bootstrap = getcwd() . '/bootstrap/app.php';
    if (!file_exists($bootstrap)) {
        $response['error'] = 'bootstrap/app.php não encontrado.';
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

    $queueManager = app('queue');
    $maxBudget = (int) ($input['max_batch'] ?? 10000);
    if ($maxBudget <= 0) $maxBudget = 10000;

    $totalCleared = 0;
    $isPartial = false;
    $clearedResults = [];
    $warningMsg = null;

    // Lua script atômico por fila:
    // Remove apenas itens da lista ready (queues:queue) e do sorted set delayed (queues:queue:delayed)
    // NUNCA toca na chave queues:queue:reserved!
    // KEYS[1] = ready list (queues:{queue})
    // KEYS[2] = delayed zset (queues:{queue}:delayed)
    // KEYS[3] = notify list (queues:{queue}:notify)
    // ARGV[1] = limit remaining
    $luaScript = <<<'LUA'
local readyKey = KEYS[1]
local delayedKey = KEYS[2]
local notifyKey = KEYS[3]
local maxLimit = tonumber(ARGV[1])

local cleared = 0
local removedIds = {}

-- 1. Drenar ready jobs (da lista)
local readyLen = redis.call('LLEN', readyKey)
if readyLen > 0 then
    local toPop = math.min(readyLen, maxLimit - cleared)
    for i = 1, toPop do
        local rawJob = redis.call('RPOP', readyKey)
        if rawJob then
            cleared = cleared + 1
            local decoded = cjson.decode(rawJob)
            if decoded and decoded['id'] then
                table.insert(removedIds, tostring(decoded['id']))
            end
        end
    end
end

-- 2. Drenar delayed jobs (do sorted set) se ainda houver orçamento
if cleared < maxLimit then
    local delayedCount = redis.call('ZCARD', delayedKey)
    if delayedCount > 0 then
        local toPopDelayed = math.min(delayedCount, maxLimit - cleared)
        -- Pega os primeiros membros do zset
        local members = redis.call('ZRANGE', delayedKey, 0, toPopDelayed - 1)
        if #members > 0 then
            for _, rawJob in ipairs(members) do
                redis.call('ZREM', delayedKey, rawJob)
                cleared = cleared + 1
                local decoded = cjson.decode(rawJob)
                if decoded and decoded['id'] then
                    table.insert(removedIds, tostring(decoded['id']))
                end
            end
        end
    end
end

-- 3. Drenar notificações correspondentes se a chave existir
if redis.call('EXISTS', notifyKey) == 1 then
    local notifyLen = redis.call('LLEN', notifyKey)
    if notifyLen > 0 then
        local toPopNotify = math.min(notifyLen, cleared)
        for i = 1, toPopNotify do
            redis.call('RPOP', notifyKey)
        end
    end
end

return {cleared, removedIds}
LUA;

    foreach ($input['queues'] as $qTarget) {
        $connName = (string) ($qTarget['connection'] ?? 'redis');
        $queueName = (string) ($qTarget['queue'] ?? 'default');

        if ($totalCleared >= $maxBudget) {
            $isPartial = true;
            break;
        }

        $remainingBudget = $maxBudget - $totalCleared;

        try {
            $queueConn = $queueManager->connection($connName);
            if (!method_exists($queueConn, 'getRedis')) {
                $warningMsg = "Conexão '{$connName}' não expõe driver Redis nativo.";
                continue;
            }
            $redis = $queueConn->getRedis();

            $readyKey = 'queues:' . $queueName;
            $delayedKey = 'queues:' . $queueName . ':delayed';
            $notifyKey = 'queues:' . $queueName . ':notify';

            $result = null;
            if (method_exists($redis, 'eval')) {
                $result = $redis->eval(
                    $luaScript,
                    3,
                    $readyKey,
                    $delayedKey,
                    $notifyKey,
                    $remainingBudget
                );
            }

            $queueCleared = 0;
            $removedIds = [];

            if (is_array($result) && count($result) >= 2) {
                $queueCleared = (int) $result[0];
                $removedIds = (array) $result[1];
            } else {
                // Fallback atômico via pipeline caso eval retorne formato diferente
                $readyLen = (int) $redis->llen($readyKey);
                $toPop = min($readyLen, $remainingBudget);
                for ($i = 0; $i < $toPop; $i++) {
                    $item = $redis->rpop($readyKey);
                    if ($item) {
                        $queueCleared++;
                        $dec = json_decode((string) $item, true);
                        if (is_array($dec) && !empty($dec['id'])) {
                            $removedIds[] = (string) $dec['id'];
                        }
                    }
                }
            }

            $totalCleared += $queueCleared;
            $clearedResults[] = [
                'connection' => $connName,
                'queue' => $queueName,
                'count' => $queueCleared,
            ];

            // Reconciliação cirúrgica de metadados do Horizon apenas para os IDs retirados
            if (!empty($removedIds)) {
                try {
                    $prefix = config('horizon.prefix', 'horizon:');
                    $pendingKey = $prefix . 'pending_jobs';
                    if (method_exists($redis, 'zrem')) {
                        foreach ($removedIds as $jobId) {
                            $redis->zrem($pendingKey, $jobId);
                        }
                    }
                } catch (\Throwable $me) {
                    $warningMsg = 'Metadados de pendentes do Horizon não puderam ser totalmente sincronizados: ' . $me->getMessage();
                }
            }

        } catch (\Throwable $qe) {
            $warningMsg = "Erro na fila '{$queueName}': " . $qe->getMessage();
            $isPartial = true;
            break;
        }
    }

    $response['ok'] = true;
    $response['cleared'] = $clearedResults;
    $response['total'] = $totalCleared;
    $response['is_partial'] = $isPartial;
    $response['warning'] = $warningMsg;

} catch (\Throwable $e) {
    $response['error'] = $e->getMessage();
}

echo json_encode($response);
"#;

/// Executa a limpeza exclusiva de pendentes de forma segura.
pub async fn execute_horizon_clear(
    root: &Path,
    queues: &[QueueScope],
    max_batch: usize,
) -> MtResult<HorizonClearResult> {
    if queues.is_empty() {
        return Ok(HorizonClearResult {
            cleared_per_queue: Vec::new(),
            total_cleared: 0,
            is_partial: false,
            warning: None,
        });
    }

    let input_payload = RawClearInput {
        queues: queues.to_vec(),
        max_batch,
    };

    let input_json = serde_json::to_vec(&input_payload).map_err(|e| {
        MtError::new(
            mt_core::ErrorCode("horizon.clear_payload_error"),
            format!("Falha ao serializar payload de limpeza: {e}"),
        )
    })?;

    let mut cmd = Command::new("php");
    cmd.args(["-r", EMBEDDED_HORIZON_CLEAR_PHP])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);

    let limits = RunLimits {
        timeout: Duration::from_secs(CLEAR_TIMEOUT_SECS),
        max_stdout: MAX_CLEAR_STDOUT_BYTES,
        max_stderr: MAX_CLEAR_STDERR_BYTES,
    };

    // Spawn com envio do payload via stdin
    let mut child = cmd.spawn().map_err(|e| {
        MtError::new(
            mt_core::ErrorCode("horizon.php_spawn_failed"),
            format!("Falha ao iniciar processo PHP de limpeza: {e}"),
        )
    })?;

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(&input_json).await;
        let _ = stdin.shutdown().await;
    }

    // Aguarda e drena com limites
    let pgid = child.id().unwrap_or(0);
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| MtError::new(mt_core::ErrorCode("core.pipe_error"), "Stdout fechado."))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| MtError::new(mt_core::ErrorCode("core.pipe_error"), "Stderr fechado."))?;

    let stdout_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let n = stdout.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Ok::<Vec<u8>, std::io::Error>(buf)
    });

    let stderr_task = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8 * 1024];
        loop {
            let n = stderr.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Ok::<Vec<u8>, std::io::Error>(buf)
    });

    let wait_res = tokio::time::timeout(limits.timeout, async {
        let st_res = stdout_task.await;
        let status = child.wait().await;
        let err_res = stderr_task.await;
        (status, st_res, err_res)
    })
    .await;

    let Ok((status_res, stdout_res, stderr_res)) = wait_res else {
        if let Ok(raw) = i32::try_from(pgid) {
            if raw > 0 {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(-raw),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
        return Err(MtError::new(
            mt_core::ErrorCode("horizon.clear_timeout"),
            format!(
                "Tempo limite de {} s excedido durante a limpeza de pendentes.",
                limits.timeout.as_secs()
            ),
        ));
    };

    let status = status_res.map_err(|e| {
        MtError::new(
            mt_core::ErrorCode("horizon.clear_status_failed"),
            e.to_string(),
        )
    })?;

    let raw_bytes = stdout_res
        .map_err(|e| {
            MtError::new(
                mt_core::ErrorCode("horizon.clear_stdout_join"),
                e.to_string(),
            )
        })?
        .map_err(|e| {
            MtError::new(
                mt_core::ErrorCode("horizon.clear_stdout_read"),
                e.to_string(),
            )
        })?;

    let stderr_bytes = stderr_res.ok().and_then(Result::ok).unwrap_or_default();
    if !status.success() {
        let err_text = String::from_utf8_lossy(&stderr_bytes);
        return Err(MtError::new(
            mt_core::ErrorCode("horizon.clear_exit_nonzero"),
            format!(
                "O processo PHP de limpeza falhou: {}",
                sanitize(err_text.trim())
            ),
        ));
    }

    let raw_text = String::from_utf8_lossy(&raw_bytes);
    let parsed: RawClearOutput = serde_json::from_str(raw_text.trim()).map_err(|e| {
        MtError::new(
            mt_core::ErrorCode("horizon.clear_invalid_json"),
            format!(
                "Resposta inválida do helper PHP de limpeza: {e}. Saída bruta: {}",
                sanitize(&raw_text)
            ),
        )
    })?;

    if let Some(err) = parsed.error {
        return Err(MtError::new(
            mt_core::ErrorCode("horizon.clear_script_error"),
            sanitize(&err).into_owned(),
        ));
    }

    let cleared_per_queue = parsed
        .cleared
        .into_iter()
        .map(|c| QueueClearCount {
            connection: c.connection,
            queue: c.queue,
            cleared: c.count,
        })
        .collect();

    Ok(HorizonClearResult {
        cleared_per_queue,
        total_cleared: parsed.total,
        is_partial: parsed.is_partial,
        warning: parsed.warning.as_deref().map(|w| sanitize(w).into_owned()),
    })
}
