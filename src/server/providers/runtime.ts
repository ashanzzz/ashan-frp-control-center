import { spawn, execFile } from 'node:child_process'
import { appendFileSync, copyFileSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { promisify } from 'node:util'
import { emitEvent } from '../events.ts'
import { getSetting, setSetting } from '../state.ts'
import { nowIso, sha256, sleep, text } from '../util.ts'
import { ProviderError } from '../provider-http.ts'

const execFileAsync = promisify(execFile)
let child: ReturnType<typeof spawn> | null = null
let processStartedAt: string | null = null
let state: 'stopped'|'starting'|'running'|'stopping'|'crashed' = 'stopped'
let lastExit: { code:number|null; signal:string|null; at:string; reason:string } | null = null
let restartTimer: NodeJS.Timeout | null = null
let stoppingForAction = false
let restartFailures = 0
let initializationStarted = false

export function configPath(){return resolve(text(getSetting('runtime.config_path',process.env.FRPC_CONFIG_PATH||'/data/frpc/conf/frpc.toml')))}
export function backupDir(){return resolve(text(getSetting('runtime.backup_dir',process.env.FRPC_BACKUP_DIR||'/data/backups/frpc')))}
export function binaryPath(){return resolve(text(getSetting('runtime.binary_path',process.env.FRPC_BINARY_PATH||'/usr/local/bin/frpc')))}
export function logPath(){return resolve(text(getSetting('runtime.log_path',process.env.FRPC_LOG_PATH||'/data/frpc/logs/frpc.log')))}
export function autoStartEnabled(){return Boolean(getSetting('runtime.autostart',true))}
export function autoRestartEnabled(){return Boolean(getSetting('runtime.auto_restart',true))}

function ensureDirs(){mkdirSync(dirname(configPath()),{recursive:true});mkdirSync(backupDir(),{recursive:true});mkdirSync(dirname(logPath()),{recursive:true})}
function writeLog(line:string){ensureDirs();appendFileSync(logPath(),`${new Date().toISOString()} ${line.replace(/\r?\n$/,'')}\n`,{encoding:'utf8',mode:0o600})}
function readTail(file:string,lines=200){if(!existsSync(file))return '';const content=readFileSync(file,'utf8');return content.split(/\r?\n/).slice(-Math.max(1,Math.min(2000,lines))).join('\n')}

export function readConfig(){const path=configPath();return {path,exists:existsSync(path),content:existsSync(path)?readFileSync(path,'utf8'):'',hash:existsSync(path)?sha256(readFileSync(path)):''}}
export function validateConfig(content:string){const errors:string[]=[];if(!content.trim())errors.push('配置为空');if(!/(serverAddr|server_addr|serverPort|server_port)/.test(content))errors.push('缺少 serverAddr/serverPort');if(!/(\[\[proxies\]\]|\[.+\])/.test(content))errors.push('没有发现代理段');if(/\x00/.test(content))errors.push('包含非法 NUL 字符');return {valid:errors.length===0,errors,proxyCount:(content.match(/\[\[proxies\]\]/g)||[]).length||Math.max(0,(content.match(/^\[[^\]]+\]/gm)||[]).length-1)}}

async function command(args:string[],timeout=15000){const bin=binaryPath();if(!existsSync(bin))throw new ProviderError('FRPC_BINARY_MISSING',`未找到内置 frpc：${bin}`,500,{binaryPath:bin});try{const result=await execFileAsync(bin,args,{timeout,maxBuffer:2*1024*1024,env:{...process.env}});return {stdout:text(result.stdout),stderr:text(result.stderr)}}catch(error:any){throw new ProviderError('FRPC_COMMAND_FAILED',text(error?.stderr)||text(error?.stdout)||error?.message||'frpc 命令执行失败',400,{args,code:error?.code,signal:error?.signal})}}
export async function version(){try{const result=await command(['--version'],5000);return result.stdout||result.stderr||'unknown'}catch(error){return error instanceof Error?`unavailable: ${error.message}`:'unavailable'}}
export async function verifyConfigFile(path=configPath()){const result=await command(['verify','-c',path],15000);return {valid:true,message:result.stdout||result.stderr||'configuration valid'}}

export async function installConfig(content:string,nodeName:string){
  const basic=validateConfig(content);if(!basic.valid)throw new ProviderError('FRPC_CONFIG_INVALID',basic.errors.join('；'),400,basic)
  const path=configPath(),dir=dirname(path),backups=backupDir();mkdirSync(dir,{recursive:true});mkdirSync(backups,{recursive:true})
  const temp=`${path}.tmp-${process.pid}-${Date.now()}`;writeFileSync(temp,content,{mode:0o600})
  try{await verifyConfigFile(temp)}catch(error){rmSync(temp,{force:true});throw error}
  let backup:string|null=null;if(existsSync(path)){backup=resolve(backups,`frpc-${new Date().toISOString().replace(/[:.]/g,'-')}.toml`);copyFileSync(path,backup)}
  renameSync(temp,path)
  return {installed:true,path,backup,nodeName,hash:sha256(content),validation:basic,verifiedByBinary:true,installedAt:nowIso()}
}
export async function restoreConfig(backup:string){if(!backup||!existsSync(backup))throw new ProviderError('FRPC_BACKUP_MISSING','回滚备份不存在',400,{backup});await verifyConfigFile(backup);copyFileSync(backup,configPath());return {restored:true,backup,path:configPath()}}

function scheduleRestart(reason:string){
  if(!autoRestartEnabled()||!autoStartEnabled()||restartTimer||stoppingForAction)return
  restartFailures+=1
  const delay=Math.min(60000,Math.max(2000,Math.pow(2,Math.min(restartFailures,5))*1000))
  writeLog(`[supervisor] frpc exited; restart in ${delay}ms (${reason})`)
  emitEvent('runtime.restart_scheduled',{delay,reason,attempt:restartFailures})
  restartTimer=setTimeout(async()=>{restartTimer=null;try{await startProcess(false)}catch(error){writeLog(`[supervisor] restart failed: ${error instanceof Error?error.message:String(error)}`);scheduleRestart('restart_failed')}},delay)
}

async function waitForSpawn(proc:ReturnType<typeof spawn>){await new Promise<void>((resolvePromise,reject)=>{const onSpawn=()=>{cleanup();resolvePromise()};const onError=(error:Error)=>{cleanup();reject(error)};const cleanup=()=>{proc.off('spawn',onSpawn);proc.off('error',onError)};proc.once('spawn',onSpawn);proc.once('error',onError)})}

async function startProcess(enableAutostart=true){
  if(child&&child.exitCode===null&&!child.killed)return runtimeStatus()
  if(enableAutostart)setSetting('runtime.autostart',true)
  ensureDirs();const file=readConfig();if(!file.exists)throw new ProviderError('FRPC_CONFIG_MISSING',`配置不存在：${file.path}`,400)
  const basic=validateConfig(file.content);if(!basic.valid)throw new ProviderError('FRPC_CONFIG_INVALID',basic.errors.join('；'),400,basic)
  await verifyConfigFile(file.path)
  stoppingForAction=false;state='starting';writeLog(`[supervisor] starting ${binaryPath()} -c ${file.path}`)
  const proc=spawn(binaryPath(),['-c',file.path],{cwd:dirname(file.path),env:{...process.env},stdio:['ignore','pipe','pipe']});child=proc
  proc.stdout?.on('data',(chunk)=>writeLog(`[stdout] ${String(chunk).trimEnd()}`))
  proc.stderr?.on('data',(chunk)=>writeLog(`[stderr] ${String(chunk).trimEnd()}`))
  proc.on('exit',(code,signal)=>{const intentional=stoppingForAction;lastExit={code,signal,at:nowIso(),reason:intentional?'intentional_stop':'unexpected_exit'};child=null;processStartedAt=null;state=intentional?'stopped':'crashed';writeLog(`[supervisor] exited code=${code} signal=${signal||''} intentional=${intentional}`);emitEvent('runtime.exited',lastExit);if(!intentional)scheduleRestart(`code=${code} signal=${signal||''}`)})
  await waitForSpawn(proc);processStartedAt=nowIso();state='running';restartFailures=0;writeLog(`[supervisor] started pid=${proc.pid}`);emitEvent('runtime.started',{pid:proc.pid,at:processStartedAt});await sleep(250)
  if(proc.exitCode!==null)throw new ProviderError('FRPC_START_FAILED',`frpc 启动后立即退出，code=${proc.exitCode}`,500,{lastExit,logs:readTail(logPath(),80)})
  return runtimeStatus()
}

async function stopProcess(disableAutostart=true){
  if(disableAutostart)setSetting('runtime.autostart',false)
  if(restartTimer){clearTimeout(restartTimer);restartTimer=null}
  if(!child||child.exitCode!==null){child=null;processStartedAt=null;state='stopped';return runtimeStatus()}
  stoppingForAction=true;state='stopping';const proc=child;writeLog(`[supervisor] stopping pid=${proc.pid}`);proc.kill('SIGTERM')
  const exited=await Promise.race([new Promise<boolean>((resolvePromise)=>proc.once('exit',()=>resolvePromise(true))),sleep(8000).then(()=>false)])
  if(!exited&&proc.exitCode===null){writeLog(`[supervisor] SIGTERM timeout; sending SIGKILL pid=${proc.pid}`);proc.kill('SIGKILL');await Promise.race([new Promise<void>((resolvePromise)=>proc.once('exit',()=>resolvePromise())),sleep(2000)])}
  stoppingForAction=false;if(!child)state='stopped';return runtimeStatus()
}

export async function action(kind:'start'|'stop'|'restart'){
  if(kind==='start')return startProcess(true)
  if(kind==='stop')return stopProcess(true)
  setSetting('runtime.autostart',true);await stopProcess(false);await sleep(300);return startProcess(false)
}

export async function runtimeStatus(){
  const file=readConfig();let binaryVersion='unknown';try{binaryVersion=await version()}catch{}
  let binaryStat:any=null;try{const s=statSync(binaryPath());binaryStat={size:s.size,mtime:s.mtime.toISOString()}}catch{}
  const running=!!child&&child.exitCode===null&&!child.killed
  return {
    mode:'embedded',
    process:{state:running?'running':state,pid:running?child?.pid:null,startedAt:processStartedAt,uptimeSeconds:processStartedAt?Math.max(0,Math.floor((Date.now()-new Date(processStartedAt).getTime())/1000)):0,desiredState:autoStartEnabled()?'running':'stopped',autoRestart:autoRestartEnabled(),lastExit},
    binary:{path:binaryPath(),exists:existsSync(binaryPath()),version:binaryVersion,...binaryStat},
    file:{...file,content:undefined},
    validation:file.exists?validateConfig(file.content):{valid:false,errors:['配置不存在'],proxyCount:0},
    log:{path:logPath(),exists:existsSync(logPath())}
  }
}
export function runtimeLogs(lines=300){return {path:logPath(),lines:Math.max(1,Math.min(2000,lines)),content:readTail(logPath(),lines)}}

export async function initializeRuntime(){
  if(initializationStarted)return;initializationStarted=true;ensureDirs()
  if(!autoStartEnabled()){state='stopped';writeLog('[supervisor] autostart disabled');return}
  const file=readConfig();if(!file.exists){state='stopped';writeLog(`[supervisor] autostart waiting for config: ${file.path}`);return}
  try{await startProcess(false)}catch(error){state='crashed';writeLog(`[supervisor] initial start failed: ${error instanceof Error?error.message:String(error)}`);scheduleRestart('initial_start_failed')}
}
export async function shutdownRuntime(){if(restartTimer){clearTimeout(restartTimer);restartTimer=null}await stopProcess(false)}
