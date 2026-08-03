import { route, publicRoute, setCookie, clearCookie } from './router.ts'
import { addSse } from './events.ts'
import { setupRequired, createAdmin, login, logout, changePassword, verifyAdminPassword, authenticateRequest } from './session.ts'
import { audit, deleteCredential, getCredential, getSetting, listCredentials, listSettings, markCredential, putCredential, setSetting } from './state.ts'
import { all, one, run } from './db.ts'
import { config } from './config.ts'
import { decrypt, encrypt } from './security.ts'
import { id, json, maskSecret, nowIso, text } from './util.ts'
import { testUnraid, containers, mutationFields, graphql } from './providers/unraid.ts'
import { ensureAuth, refreshToken, startDeviceFlow, pollDevice, manualTokens, listNodesRaw, listTunnelsRaw, validateLogin } from './providers/chmlfrp.ts'
import { testCloudflare, zones } from './providers/cloudflare.ts'
import { readConfig, runtimeStatus, runtimeLogs, validateConfig } from './providers/runtime.ts'
import { syncNodes, testNode, testAllNodes, listNodes, clearBan } from './services/nodes.ts'
import { listDesiredTunnels, listObservedTunnels, saveDesiredTunnel, deleteDesiredTunnel, syncObservedTunnels, buildTunnelPlan } from './services/tunnels.ts'
import { listDesiredDns, listObservedDns, saveDesiredDns, deleteDesiredDns, syncObservedDns, buildDnsPlan, claimDns, deriveDns } from './services/dns.ts'
import { healthCheck, latestHealth } from './services/health.ts'
import { buildSwitchPlan, listSwitchPlans } from './services/switch.ts'
import { cancelJob, enqueueJob, getJob, listJobs, retryJob } from './jobs.ts'

function requireText(value:unknown,name:string){const out=text(value);if(!out)throw Object.assign(new Error(`${name}不能为空`),{code:'INPUT_REQUIRED',status:400});return out}
function safeSettings(){const data=listSettings();for(const key of Object.keys(data)){if(/password|token|secret|api_key/i.test(key))delete data[key]}return data}
function currentSession(req:any){return authenticateRequest(req)}

publicRoute('GET','/api/v1/auth/status',({req})=>({setupRequired:setupRequired(),authenticated:!!currentSession(req)}))
publicRoute('POST','/api/v1/auth/setup',({req,res,body,requestId})=>{const user=createAdmin(requireText(body?.username,'用户名'),requireText(body?.password,'密码'),text(body?.displayName));const result=login(user.username,body.password,req);setCookie(res,'ashan_frp_session',result.token,config.sessionTtlHours*3600);audit(user.id,'auth.setup','admin',user.id,'success',{},requestId);return result.session})
publicRoute('POST','/api/v1/auth/login',({req,res,body,requestId})=>{const result=login(requireText(body?.username,'用户名'),requireText(body?.password,'密码'),req);setCookie(res,'ashan_frp_session',result.token,config.sessionTtlHours*3600);audit(result.session.user.id,'auth.login','session',result.session.id,'success',{},requestId);return result.session})
route('GET','/api/v1/auth/session',({user})=>({user:{id:user.id,username:user.username,displayName:user.displayName},csrfToken:user.csrfToken,expiresAt:user.expiresAt}))
route('POST','/api/v1/auth/logout',({req,res,user,requestId})=>{logout(req);clearCookie(res,'ashan_frp_session');audit(user.id,'auth.logout','session',user.sessionId,'success',{},requestId);return{loggedOut:true}})
route('POST','/api/v1/auth/change-password',({body,user,requestId})=>{changePassword(user.id,requireText(body?.currentPassword,'当前密码'),requireText(body?.newPassword,'新密码'));audit(user.id,'auth.password.change','admin',user.id,'success',{},requestId);return{changed:true,reloginRequired:true}})
route('GET','/api/v1/events',({res})=>{addSse(res);return null},{csrf:false})

route('GET','/api/v1/system/health',async()=>latestHealth()||await healthCheck(),{csrf:false})
route('POST','/api/v1/system/health/run',({user})=>enqueueJob('health.check',{}, {requestedBy:user.id,priority:80,maxAttempts:1}))
route('GET','/api/v1/system/dashboard',async()=>{const health=latestHealth();const counts={nodes:Number(one('SELECT COUNT(*) AS n FROM nodes')?.n||0),onlineNodes:Number(one('SELECT COUNT(*) AS n FROM nodes WHERE online=1')?.n||0),desiredTunnels:Number(one('SELECT COUNT(*) AS n FROM desired_tunnels WHERE enabled=1')?.n||0),observedTunnels:Number(one('SELECT COUNT(*) AS n FROM observed_tunnels')?.n||0),dns:Number(one('SELECT COUNT(*) AS n FROM desired_dns WHERE enabled=1')?.n||0),activeJobs:Number(one(`SELECT COUNT(*) AS n FROM jobs WHERE status IN ('queued','running','retry_wait')`)?.n||0)};let runtime=null;try{runtime=await runtimeStatus()}catch{}return{health,counts,currentNode:getSetting('runtime.current_node',''),runtime,automation:{enabled:getSetting('automation.enabled',false),lastPlanJob:getSetting('automation.last_plan_job','')},recentJobs:listJobs(8),recentSwitches:listSwitchPlans().slice(0,5)}})

route('GET','/api/v1/settings',()=>safeSettings(),{csrf:false})
route('PATCH','/api/v1/settings',({body,user,requestId})=>{for(const [key,value] of Object.entries(body||{})){if(/password|token|secret|api_key/i.test(key))continue;setSetting(key,value,user.id)}audit(user.id,'settings.update','settings',null,'success',{keys:Object.keys(body||{})},requestId);return safeSettings()})

route('GET','/api/v1/credentials',()=>listCredentials(),{csrf:false})
route('PUT','/api/v1/credentials/:provider/:name',({params,body,user,requestId})=>{const result=putCredential(params.provider,params.name,requireText(body?.secret,'密钥'),body?.metadata||{});audit(user.id,'credential.save','credential',`${params.provider}/${params.name}`,'success',{revision:result.revision},requestId);return result})
route('POST','/api/v1/credentials/:provider/:name/reveal',({params,body,user,requestId})=>{if(!verifyAdminPassword(user.id,requireText(body?.password,'管理员密码')))throw Object.assign(new Error('管理员密码错误'),{code:'PASSWORD_INVALID',status:403});const value=getCredential(params.provider,params.name);if(!value)throw Object.assign(new Error('凭据不存在'),{code:'CREDENTIAL_NOT_FOUND',status:404});audit(user.id,'credential.reveal','credential',`${params.provider}/${params.name}`,'success',{},requestId);return{value,expiresIn:30}})
route('DELETE','/api/v1/credentials/:provider/:name',({params,user,requestId})=>{deleteCredential(params.provider,params.name);audit(user.id,'credential.delete','credential',`${params.provider}/${params.name}`,'success',{},requestId);return{deleted:true}})

route('POST','/api/v1/providers/unraid/test',async({user})=>{const result=await testUnraid();markCredential('unraid','api_key',true);return result})
route('GET','/api/v1/providers/unraid/containers',()=>containers(),{csrf:false})
route('GET','/api/v1/providers/unraid/mutations',()=>mutationFields(),{csrf:false})
route('POST','/api/v1/providers/unraid/graphql',({body})=>graphql(requireText(body?.query,'GraphQL Query'),body?.variables||{}))

route('POST','/api/v1/providers/chmlfrp/test',async()=>{const result=await validateLogin();markCredential('chmlfrp','access_token',true);return result})
route('GET','/api/v1/providers/chmlfrp/auth-status',async()=>{try{return{authenticated:true,mode:'access',data:await validateLogin()}}catch(e){return{authenticated:false,error:e instanceof Error?e.message:String(e),accessTokenConfigured:!!getCredential('chmlfrp','access_token'),refreshTokenConfigured:!!getCredential('chmlfrp','refresh_token')}}},{csrf:false})
route('POST','/api/v1/providers/chmlfrp/auth/ensure',({user})=>enqueueJob('auth.ensure',{}, {requestedBy:user.id,priority:95,maxAttempts:1}))
route('POST','/api/v1/providers/chmlfrp/auth/refresh',({user})=>enqueueJob('auth.refresh',{}, {requestedBy:user.id,priority:95,maxAttempts:1}))
route('POST','/api/v1/providers/chmlfrp/auth/manual',({body,user,requestId})=>{const result=manualTokens(requireText(body?.accessToken,'Access Token'),text(body?.refreshToken)||undefined,text(body?.expiresAt)||undefined);audit(user.id,'chmlfrp.auth.manual','credential','chmlfrp/access_token','success',{},requestId);return result})
route('POST','/api/v1/providers/chmlfrp/auth/device/start',async({user})=>{const data=await startDeviceFlow(),challengeId=id(),now=nowIso(),expires=new Date(Date.now()+Number(data.expires_in||600)*1000).toISOString();run('INSERT INTO oauth_challenges(id,provider,kind,status,session_tag,payload_ciphertext,expires_at,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?,?)',challengeId,'chmlfrp','device','pending',text(data.user_code),encrypt(JSON.stringify(data)),expires,now,now);audit(user.id,'chmlfrp.device.start','oauth_challenge',challengeId,'success',{userCode:data.user_code});return{challengeId,userCode:data.user_code,verificationUri:data.verification_uri||data.verification_uri_complete,expiresAt:expires,interval:data.interval||5}})
route('POST','/api/v1/providers/chmlfrp/auth/device/poll',async({body})=>{const row=one('SELECT * FROM oauth_challenges WHERE id=?',requireText(body?.challengeId,'challengeId'));if(!row)throw Object.assign(new Error('授权会话不存在'),{code:'CHALLENGE_NOT_FOUND',status:404});const payload=JSON.parse(decrypt(row.payload_ciphertext));const result=await pollDevice(payload.device_code);run('UPDATE oauth_challenges SET status=\'completed\',updated_at=? WHERE id=?',nowIso(),row.id);return result})
route('GET','/api/v1/providers/chmlfrp/raw/nodes',()=>listNodesRaw(),{csrf:false})
route('GET','/api/v1/providers/chmlfrp/raw/tunnels',()=>listTunnelsRaw(),{csrf:false})
route('POST','/api/v1/providers/cloudflare/test',()=>testCloudflare())
route('GET','/api/v1/providers/cloudflare/zones',()=>zones(),{csrf:false})

route('GET','/api/v1/nodes',()=>listNodes(),{csrf:false})
route('POST','/api/v1/nodes/sync',({user})=>enqueueJob('nodes.sync',{}, {requestedBy:user.id,priority:70,maxAttempts:2}))
route('POST','/api/v1/nodes/test-all',({user})=>enqueueJob('nodes.sync',{}, {requestedBy:user.id,priority:70,maxAttempts:1,idempotencyKey:`nodes-test-${Math.floor(Date.now()/30000)}`}))
route('POST','/api/v1/nodes/:name/test',({params})=>testNode(params.name))
route('POST','/api/v1/nodes/:name/unban',({params})=>{clearBan(params.name);return{cleared:true}})
route('POST','/api/v1/nodes/:name/switch-plan',({params,body,user})=>buildSwitchPlan(params.name,text(body?.reason)||'manual'))

route('GET','/api/v1/tunnels',()=>({desired:listDesiredTunnels(),observed:listObservedTunnels()}),{csrf:false})
route('POST','/api/v1/tunnels',({body,user,requestId})=>{const result=saveDesiredTunnel(body);audit(user.id,'tunnel.desired.save','desired_tunnel',result.id,'success',{name:result.name},requestId);return result})
route('PUT','/api/v1/tunnels/:id',({params,body,user,requestId})=>{const result=saveDesiredTunnel({...body,id:params.id});audit(user.id,'tunnel.desired.update','desired_tunnel',params.id,'success',{name:result.name},requestId);return result})
route('DELETE','/api/v1/tunnels/:id',({params,user,requestId})=>{deleteDesiredTunnel(params.id);audit(user.id,'tunnel.desired.delete','desired_tunnel',params.id,'success',{},requestId);return{deleted:true}})
route('POST','/api/v1/tunnels/sync',({user})=>enqueueJob('tunnels.sync',{}, {requestedBy:user.id,priority:70,maxAttempts:2}))
route('POST','/api/v1/tunnels/plan',async({body})=>{await syncObservedTunnels();return buildTunnelPlan(text(body?.targetNode)||undefined,{cleanupOrphans:!!body?.cleanupOrphans})})
route('POST','/api/v1/tunnels/apply',({body,user})=>enqueueJob('tunnels.reconcile',{targetNode:text(body?.targetNode)||undefined,cleanupOrphans:!!body?.cleanupOrphans,dryRun:false},{requestedBy:user.id,priority:90,maxAttempts:1}))

route('GET','/api/v1/dns',()=>({desired:listDesiredDns(),observed:listObservedDns()}),{csrf:false})
route('POST','/api/v1/dns',({body,user,requestId})=>{const result=saveDesiredDns(body);audit(user.id,'dns.desired.save','desired_dns',result.id,'success',{name:result.name},requestId);return result})
route('PUT','/api/v1/dns/:id',({params,body,user,requestId})=>{const result=saveDesiredDns({...body,id:params.id});audit(user.id,'dns.desired.update','desired_dns',params.id,'success',{name:result.name},requestId);return result})
route('DELETE','/api/v1/dns/:id',({params,user,requestId})=>{const result=deleteDesiredDns(params.id);audit(user.id,'dns.desired.delete','desired_dns',params.id,'success',{},requestId);return result})
route('POST','/api/v1/dns/derive',({body})=>deriveDns(text(body?.targetNode)||text(getSetting('runtime.current_node',''))))
route('POST','/api/v1/dns/sync',({user})=>enqueueJob('dns.sync',{}, {requestedBy:user.id,priority:70,maxAttempts:2}))
route('POST','/api/v1/dns/plan',async({body})=>{await syncObservedDns();return buildDnsPlan({cleanupOrphans:!!body?.cleanupOrphans})})
route('POST','/api/v1/dns/apply',({body,user})=>enqueueJob('dns.reconcile',{targetNode:text(body?.targetNode)||undefined,cleanupOrphans:!!body?.cleanupOrphans,dryRun:false},{requestedBy:user.id,priority:90,maxAttempts:1}))
route('POST','/api/v1/dns/:externalId/claim',({params,body})=>claimDns(params.externalId,body?.managed!==false))

route('GET','/api/v1/runtime',()=>runtimeStatus(),{csrf:false})
route('GET','/api/v1/runtime/config',()=>{const data=readConfig();return{...data,validation:data.exists?validateConfig(data.content):{valid:false,errors:['配置不存在'],proxyCount:0}}},{csrf:false})
route('GET','/api/v1/runtime/logs',({query})=>runtimeLogs(Number(query.get('lines')||300)),{csrf:false})
route('POST','/api/v1/runtime/action',({body,user})=>enqueueJob('runtime.action',{action:requireText(body?.action,'动作')},{requestedBy:user.id,priority:90,maxAttempts:1}))

route('GET','/api/v1/switch-plans',()=>listSwitchPlans(),{csrf:false})
route('POST','/api/v1/switch-plans',({body,user})=>buildSwitchPlan(text(body?.targetNode)||undefined,text(body?.reason)||'manual'))
route('POST','/api/v1/switch-plans/:id/execute',({params,user})=>enqueueJob('switch.execute',{planId:params.id},{requestedBy:user.id,targetType:'switch_plan',targetId:params.id,priority:100,maxAttempts:1,idempotencyKey:`switch-${params.id}`}))

route('GET','/api/v1/jobs',({query})=>listJobs(Number(query.get('limit')||100)),{csrf:false})
route('GET','/api/v1/jobs/:id',({params})=>{const job=getJob(params.id);if(!job)throw Object.assign(new Error('任务不存在'),{code:'JOB_NOT_FOUND',status:404});return job},{csrf:false})
route('POST','/api/v1/jobs/:id/cancel',({params})=>cancelJob(params.id))
route('POST','/api/v1/jobs/:id/retry',({params})=>retryJob(params.id))

route('GET','/api/v1/cache',()=>all('SELECT key,provider,status,record_count,payload_hash,updated_at,expires_at,last_error FROM cache_entries ORDER BY provider,key'),{csrf:false})
route('GET','/api/v1/cache/:key',({params})=>{const row=one('SELECT * FROM cache_entries WHERE key=?',params.key);if(!row)throw Object.assign(new Error('缓存不存在'),{code:'CACHE_NOT_FOUND',status:404});return{...row,value:json(row.value_json,null)}},{csrf:false})
route('DELETE','/api/v1/cache/:key',({params})=>{run('DELETE FROM cache_entries WHERE key=?',params.key);return{deleted:true}})
route('GET','/api/v1/audit',({query})=>all('SELECT * FROM audit_logs ORDER BY created_at DESC LIMIT ?',Math.max(1,Math.min(500,Number(query.get('limit')||200)))).map((r)=>({...r,details:json(r.details_json,{})})),{csrf:false})

route('GET','/api/v1/oauth/challenges',()=>all('SELECT id,provider,kind,status,session_tag,expires_at,created_at,updated_at FROM oauth_challenges ORDER BY created_at DESC LIMIT 100'),{csrf:false})
route('POST','/api/v1/oauth/challenges/:id/code',({params,body,user,requestId})=>{const code=requireText(body?.code,'验证码');run('UPDATE oauth_challenges SET code_ciphertext=?,status=\'code_received\',updated_at=? WHERE id=?',encrypt(code),nowIso(),params.id);audit(user.id,'oauth.code.manual','oauth_challenge',params.id,'success',{},requestId);return{received:true,mask:maskSecret(code)}})
publicRoute('POST','/api/v1/webhooks/email-code',({req,body})=>{if(!getSetting('email.webhook_enabled',false))throw Object.assign(new Error('邮件Webhook未启用'),{code:'WEBHOOK_DISABLED',status:403});const expected=getCredential('email','webhook_token'),provided=text(req.headers['x-webhook-token']);if(!expected||provided!==expected)throw Object.assign(new Error('Webhook认证失败'),{code:'WEBHOOK_UNAUTHORIZED',status:401});const challengeId=text(body?.challengeId),sessionTag=text(body?.sessionTag),code=requireText(body?.code,'验证码');let row=challengeId?one('SELECT * FROM oauth_challenges WHERE id=?',challengeId):sessionTag?one(`SELECT * FROM oauth_challenges WHERE session_tag=? AND status IN ('pending','waiting_code') ORDER BY created_at DESC LIMIT 1`,sessionTag):null;if(!row)throw Object.assign(new Error('未找到匹配的OAuth会话'),{code:'CHALLENGE_NOT_FOUND',status:404});run('UPDATE oauth_challenges SET code_ciphertext=?,status=\'code_received\',updated_at=? WHERE id=?',encrypt(code),nowIso(),row.id);return{received:true,challengeId:row.id}})
