# Server Auth And Runtime Trust

This reference captures the landed post-Firebase, post-Cloud-Functions trust
baseline for server-owned auth, provider-family compatibility seams, runtime
bootstrap ownership, and trusted metadata contracts.

It complements:

- [runtime capability and adapter boundary](runtime-adapter-boundary.md)
- [firebase application auth contract](firebase-auth-contract.md)
- [cloud functions compatibility](cloud-functions-compatibility.md)
- [adapter runtime trust hardening plan](../plans/adapter-runtime-trust-hardening-plan.md)
- [server runtime canonicalization plan](../plans/server-runtime-canonicalization-plan.md)

## Landed Conclusions

The current landed architecture now reflects the following settled rules:

1. Shared application auth is server-owned; adapters consume it rather than
   owning principal normalization or bearer verification semantics.
2. Cloud Functions callable auth fails closed when a bearer token is presented
   but cannot be verified.
3. Firestore-family compatibility logic shared by Firebase and Cloud Functions
   meets on a provider-family seam rather than through adapter-to-adapter
   imports.
4. Covered Firestore-admin metadata is truthful or omitted.
5. Shared runtime bootstrap has one authoritative implementation.
6. Shared runtime capability execution is explicitly separated from runtime
   ABI payload dispatch.

## Direction

- Server auth should be server-owned.
- Adapters may depend on shared auth and provider-family seams.
- Adapters should not depend on each other for compatibility translation.
- Shared runtime capability code should be provider-neutral and runtime-ABI
  aware only where explicitly named as such.
- Pre-launch direct corrections are preferred over compatibility shims.
