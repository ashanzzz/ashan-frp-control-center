import { resolve } from 'node:path'
import { mkdirSync } from 'node:fs'

const cwd = process.cwd()
const dataDir = resolve(process.env.DATA_DIR || resolve(cwd, 'data'))
export const config = {
  host: process.env.HTTP_HOST || '0.0.0.0',
  port: Number(process.env.HTTP_PORT || 8080),
  dataDir,
  publicDir: resolve(process.env.PUBLIC_DIR || resolve(cwd, 'public')),
  databasePath: '',
  masterKeyPath: '',
  frpcBinaryPath: resolve(process.env.FRPC_BINARY_PATH || '/usr/local/bin/frpc'),
  frpcConfigPath: resolve(process.env.FRPC_CONFIG_PATH || resolve(dataDir, 'frpc/conf/frpc.toml')),
  frpcBackupDir: resolve(process.env.FRPC_BACKUP_DIR || resolve(dataDir, 'backups/frpc')),
  frpcLogPath: resolve(process.env.FRPC_LOG_PATH || resolve(dataDir, 'frpc/logs/frpc.log')),
  sessionTtlHours: Math.max(1, Number(process.env.SESSION_TTL_HOURS || 24)),
  cookieSecure: String(process.env.COOKIE_SECURE || 'false').toLowerCase() === 'true',
  maxBodyBytes: 2 * 1024 * 1024,
}
config.databasePath = resolve(config.dataDir, 'state.db')
config.masterKeyPath = resolve(config.dataDir, 'master.key')
mkdirSync(config.dataDir, { recursive: true })
mkdirSync(config.frpcBackupDir, { recursive: true })
mkdirSync(resolve(config.dataDir,'frpc/conf'), { recursive: true })
mkdirSync(resolve(config.dataDir,'frpc/logs'), { recursive: true })
