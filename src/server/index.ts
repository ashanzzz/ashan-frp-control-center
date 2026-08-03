import './db.ts'
import './routes.ts'
import { startServer } from './router.ts'
import { startJobRunner, startScheduler, stopJobRunner } from './jobs.ts'
import { cleanupSessions } from './session.ts'
import { run } from './db.ts'
import { nowIso } from './util.ts'
import { initializeRuntime, shutdownRuntime } from './providers/runtime.ts'

cleanupSessions()
run(`DELETE FROM oauth_challenges WHERE expires_at<?`,nowIso())
startJobRunner()
startScheduler()
startServer()
void initializeRuntime()

let shuttingDown=false
async function shutdown(signal:string){
  if(shuttingDown)return
  shuttingDown=true
  console.log(`received ${signal}; stopping embedded frpc and job runner`)
  stopJobRunner()
  try{await shutdownRuntime()}catch(error){console.error('runtime shutdown failed',error)}
  process.exit(0)
}
process.on('SIGTERM',()=>void shutdown('SIGTERM'))
process.on('SIGINT',()=>void shutdown('SIGINT'))
