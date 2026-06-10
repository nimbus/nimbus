---
status: done
phase: SATH8
---

# SATH8 Lifecycle State Machine

Table lifecycle transition semantics now have a pure shared state machine. The
backend SQL/key-value writes stay backend-owned, but the legal transition model
is centralized and tested.

Evidence:

- `TableLifecycleTransition`
- `TableLifecycleStateMachine`
- `apply_table_lifecycle_transition`
- `table_lifecycle_state_machine_rejects_invalid_transitions`
- `uses_shared_table_lifecycle_transition`
