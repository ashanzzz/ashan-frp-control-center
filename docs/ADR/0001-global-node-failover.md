# ADR 0001: Global node failover

**Status:** Accepted

All managed tunnels share one Active ChmlFrp node. A confirmed node fault moves the complete managed set. Per-tunnel node assignment is forbidden because it creates mixed routing/DNS state and weakens recovery guarantees.
