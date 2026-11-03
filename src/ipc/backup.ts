import { call } from '@/ipc/core';
import { commands } from '@/types/bindings';
import type { BackupEntryDto, RestoreScheduledDto } from '@/types/bindings';

/** Lista todos os backups disponíveis no diretório `backups/`, do mais recente ao mais antigo. */
export async function listDbBackups(): Promise<BackupEntryDto[]> {
    return call('backup_list', () => commands.backupList());
}

/** Cria imediatamente um backup manual consistente usando a Online Backup API do SQLite. */
export async function createDbBackup(): Promise<BackupEntryDto> {
    return call('backup_create', () => commands.backupCreate());
}

/** Valida um arquivo de backup e agenda sua restauração atômica (`pending-restore.json`) para o próximo boot. */
export async function restoreDbBackup(backupFileName: string): Promise<RestoreScheduledDto> {
    return call('backup_restore_schedule', () => commands.backupRestoreSchedule(backupFileName));
}
