import { createCipheriv, createDecipheriv, randomBytes, scryptSync, timingSafeEqual } from 'node:crypto'
import { existsSync, readFileSync, writeFileSync, chmodSync } from 'node:fs'
import { config } from './config.ts'
import { sha256 } from './util.ts'

function masterKey(): Buffer {
  if (!existsSync(config.masterKeyPath)) { writeFileSync(config.masterKeyPath, randomBytes(32), { mode: 0o600 }); try { chmodSync(config.masterKeyPath, 0o600) } catch {} }
  const key = readFileSync(config.masterKeyPath)
  if (key.length !== 32) throw new Error('master.key必须为32字节')
  return key
}
export function encrypt(value: string): string {
  const iv=randomBytes(12), cipher=createCipheriv('aes-256-gcm', masterKey(), iv)
  const ciphertext=Buffer.concat([cipher.update(value,'utf8'),cipher.final()]), tag=cipher.getAuthTag()
  return [iv,tag,ciphertext].map((item)=>item.toString('base64url')).join('.')
}
export function decrypt(value: string): string {
  const [ivText,tagText,cipherText]=value.split('.')
  if (!ivText||!tagText||!cipherText) throw new Error('密文格式错误')
  const decipher=createDecipheriv('aes-256-gcm',masterKey(),Buffer.from(ivText,'base64url'))
  decipher.setAuthTag(Buffer.from(tagText,'base64url'))
  return Buffer.concat([decipher.update(Buffer.from(cipherText,'base64url')),decipher.final()]).toString('utf8')
}
export function hashPassword(password: string, salt = randomBytes(16).toString('base64url')): { hash:string; salt:string } {
  return { hash:scryptSync(password,salt,64).toString('base64url'), salt }
}
export function verifyPassword(password: string, salt: string, expected: string): boolean {
  const actual=scryptSync(password,salt,64), wanted=Buffer.from(expected,'base64url')
  return actual.length===wanted.length && timingSafeEqual(actual,wanted)
}
export function tokenHash(value: string): string { return sha256(value) }
