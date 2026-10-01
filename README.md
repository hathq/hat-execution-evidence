# HAT execution evidence (0.10.0)

Provider-owned adapter over `zixcel-revision`, using HatSpec's existing result
and exact-reference types. It has no Hatter, Graph, semantic compiler, network
daemon or scheduler dependency.

The protected provider owner admits an original execution, holds its private
non-serializable permit, and records a known result after the effect but before
notification. A worker must never receive the signing key or Authority object.
The host supplies authenticated worker routing and persistent key custody; this
library is not an authentication service. `accept` is a protected owner API,
not an endpoint accepting arbitrary browser/worker assertions.

An authority restart can reopen the existing revision backend and historical
public verification material. It cannot reconstruct old write permits through
replay. G2 receives read-only `lookup`, not G1's authority. No method executes an
action or retries an unknown effect. A crash between an arbitrary external
effect and durable evidence remains uncertain; exactly-once is not promised.

The execution stream commits admission and the signed, bounded result envelope.
Large result bodies remain external. Divergent signed proposals remain prepared
objects and lookup returns Conflict preserving both proof references. Reads never
initialize, repair, sign, move heads or mutate retained evidence. Current generic
receipt capacity/retention applies; expiry is signed owner policy, not permission
to replay. Backend recovery is an explicit host operation.

This library's tests alone do not certify Hatter's historical result admission
or the product G1→G2 scenario. Those remain separate integration gates.
