# v0.1.1 核心逻辑 Review

本轮 Review 以“所有隧道只允许全局统一切换”为最高不变量。

## 已修复

1. **FRPC 旧日志误触发**：每次启动增加 `runtime_generation`；旧代次日志直接丢弃。
2. **服务端 connection refused 被误判为本地服务错误**：日志分类先识别 server/control connection，再识别明确的 local service 语义。
3. **DNS 更新失败后的 Active Node 不一致**：新节点经 FRPC 全量验证后即提交为 Active，状态进入 `dns_switching`；DNS 失败转 `degraded_dns`。
4. **全局锁期间真实 FRPC 故障丢失**：自动故障事件等待全局锁，再通过 generation 判断是否仍有效。
5. **Readiness timeout 错误隔离备用节点**：只有明确 Node 级事件，或 timeout + ChmlFrp 明确报告 Offline，才隔离候选。
6. **ChmlFrp 逐条迁移半完成**：Global Failover 增加预检、3 次重试、全量远端验证；在 FRPC 提交前失败会对已修改项执行补偿恢复。
7. **Reconcile 无条件重启 FRPC**：配置完全一致且 FRPC 已 Running+Connected 时跳过重启，降低不必要中断。
8. **Reconcile 在 DNS 未配置时先修改 Provider**：现在 DNS-managed 计划会在所有写操作前检查 Cloudflare 配置和目标节点 real IP。
9. **删除本地计划留下远端孤儿资源**：只要 ChmlFrp 同名隧道或受管 Cloudflare A 记录仍存在，就拒绝删除。
10. **运行状态与 UI 状态混淆**：每条隧道仍显示 FRPC 运行状态，但状态只能作为诊断/触发信号，绝无 per-tunnel failover 路径。
11. **容器重启后 FRPC 不自动恢复**：已有二进制与持久化 ChmlFrp 配置时，Control Center 启动后自动拉起 FRPC；失败只报警，不阻塞 Web 控制台。
12. **Cloudflare 同域名多 A 记录被 HashMap 吞掉**：主控台现在显示 `A 记录冲突`；Reconcile/Failover 在任何 Provider 写操作前检查受管域名 A 记录唯一性。
13. **Release artifact glibc 兼容性**：GitHub `build.yml` 改为 `x86_64-unknown-linux-musl` 便携构建。

## 仍需真实环境验证 / 后续硬化

- **认证层尚未实现**：当前 API 只能放在可信 LAN/VPN/外部访问控制后，不能直接公网暴露。设计目标仍是单管理员 + HttpOnly 服务端 Session。
- ChmlFrp `/update_tunnel` 对 HTTP/HTTPS 的实际账号行为；Provider 拒绝时系统会停止在 DNS 之前。
- 不同 ChmlFrp/frpc 版本的实际日志文案，需要通过真实运行日志继续扩充分类测试样本。
- GitHub Actions 是最终 Rust/Dioxus 编译门禁；生成 ZIP 的本地环境没有 Rust toolchain。
