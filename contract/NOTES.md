# ros2_medkit REST contract corpus — provenance & regenerate recipe

This directory is a **frozen, verbatim capture** of the REST wire contract served
by the upstream C++ gateway [`selfpatch/ros2_medkit`](https://github.com/selfpatch/ros2_medkit).
It is the single source of truth that `taktora-medkit` model and gateway slices
(GitHub #80 onward) diff against to make "drop-in client compatible" a testable
claim. ros2_medkit publishes **no static contract file** — its OpenAPI 3.1.0 is
generated at runtime from C++ DTO reflection and served at `GET /api/v1/docs`.
Everything here was captured from the **running binary**, not from the prose docs
site (which is self-contradictory on the auth paths; see below).

> This is captured **upstream** data, not taktora prose. Do not "fix" casing,
> envelope keys, or typos in these files — the mixed snake_case / camelCase shapes
> are the contract. `contract/**` is excluded from the `typos` spell-check for this
> reason (see `typos.toml`).

## Source / provenance

| Field | Value |
| --- | --- |
| Image ref | `ghcr.io/selfpatch/ros2_medkit-jazzy:latest` |
| Image digest | `ghcr.io/selfpatch/ros2_medkit-jazzy@sha256:410a67a64874c2e1d7547729bcb4199a395b79f82645cd224fc5f9fe0561f5c8` |
| Image created | `2026-06-23T19:14:30Z` (platform `linux/amd64`) |
| Vendor version (from running binary `version-info` + OpenAPI `info.version`) | ros2_medkit **0.6.0** |
| SOVD API version (`info.x-sovd-version`, `version-info[].version`) | **1.0.0** |
| Probable upstream commit | `243a2f4aabb0433a1ad597a1f93ae8f8553c7e81` (`git describe` = `0.6.0-20-g243a2f4a`, committed 2026-06-23, `package.xml` version `0.6.0`) — matches the image's `0.6.0` build date. Pinned from the local reference checkout `/home/patdhlk/src/patdhlk/ros2_medkit`; the published image carries no source-commit label, so this is a best-effort match, not a label-verified pin. The image **digest** above is the authoritative pin. |
| Capture date | **2026-06-28** |
| Capture host | `aarch64` Linux; the amd64-only image was run under QEMU (`tonistiigi/binfmt`) x86_64 emulation. Emulation affects timing only, not wire bytes. |

## Standing up the gateway (exact invocation)

The image is **amd64-only** and has no `arm64` manifest. On an aarch64 host,
register QEMU first:

```bash
docker run --privileged --rm tonistiigi/binfmt --install amd64
```

Then run the demo (per ros2_medkit README ~line 84 — it runs **without** ROS 2):

```bash
docker pull --platform linux/amd64 ghcr.io/selfpatch/ros2_medkit-jazzy:latest
docker run -d --platform linux/amd64 --name medkit-gw \
  --network host --ipc host \
  ghcr.io/selfpatch/ros2_medkit-jazzy:latest
# → REST API live at http://localhost:8080/api/v1/
```

On a native amd64 host, drop `--platform linux/amd64` and the binfmt step.

### Fault data: a minimal ROS 2 `fault_manager` was required

The bare demo runs the gateway **without** a `fault_manager` node, so every fault
endpoint returns the `service-unavailable` GenericError
(`{"error_code":"service-unavailable","message":"Failed to get faults",...}`) —
no DTC objects, no freeze-frames, no SSE frames.

To capture the **real** fault response bodies (the DTC status sub-object and the
freeze-frame are produced entirely by the gateway's own C++ handlers
— `src/.../http/handlers/fault_handlers.cpp` `build_status_object()` /
`build_sovd_fault_response()`, and `src/.../ros2/conversions/fault_msg_conversions.cpp`),
a minimal ROS 2 mock `fault_manager` was run **inside the container** to feed the
four services the gateway's `Ros2FaultServiceTransport` binds to under
`/fault_manager/*` (`list_faults`, `get_fault`, `report_fault`, `clear_fault`) and
to publish `FaultEvent` on `/fault_manager/events` (drives `/faults/stream` SSE).
The mock only supplies the upstream `ros2_medkit_msgs` `Fault` / `EnvironmentData`
inputs; **the gateway transforms them into the captured JSON**, so the casing,
`_links`, `x-medkit`, status sub-object, and freeze-frame shapes are authentic.

The mock script is reproduced verbatim at the end of this file. Run it via:

```bash
docker cp fault_manager_mock.py medkit-gw:/tmp/fault_manager_mock.py
docker exec -d medkit-gw bash -lc \
  'source /opt/ros/jazzy/setup.bash; source /home/medkit/ws/install/setup.bash; \
   python3 /tmp/fault_manager_mock.py'
```

Fault `reporting_sources` use ROS node FQNs (e.g. `/ros2_medkit_gateway`) because
the gateway's entity-scope filter (`core/faults/fault_scope.cpp`,
`source_matches_scope`) only surfaces a fault under an entity when every reporting
source is one of that entity's owned app FQNs.

## Endpoints hit (regenerate recipe)

`B=http://localhost:8080/api/v1`. All bodies pretty-printed with `jq .` (server
emits compact JSON; **key order preserved as emitted** — `jq` does not reorder
object keys unless `-S` is passed, which it was **not** here, so ordering is the
server's own. Whitespace/indentation is the only reformatting). The SSE `.txt`
sample is the **last two live frames** of `/faults/stream` (a fresh connection
first replays the handler's retained ring buffer, so the head of a stream can
carry pre-restart events — always read the live tail).

| File | Endpoint |
| --- | --- |
| `openapi.json` | `GET /docs` (OpenAPI 3.1.0, 151 paths) |
| `golden/root.json` | `GET /` (capability + endpoint catalogue, 239 endpoints) |
| `golden/version-info.json` | `GET /version-info` |
| `golden/health.json` | `GET /health` |
| `golden/areas_list.json` | `GET /areas` (empty — no ROS 2 areas in demo) |
| `golden/components_list.json` | `GET /components` |
| `golden/component_get.json` | `GET /components/spark-6723` |
| `golden/component_hosts.json` | `GET /components/spark-6723/hosts` |
| `golden/component_depends-on.json` | `GET /components/spark-6723/depends-on` |
| `golden/component_subcomponents.json` | `GET /components/spark-6723/subcomponents` |
| `golden/apps_list.json` | `GET /apps` |
| `golden/app_get.json` | `GET /apps/ros2_medkit_gateway` |
| `golden/app_is-located-on.json` | `GET /apps/ros2_medkit_gateway/is-located-on` |
| `golden/app_belongs-to.json` | `GET /apps/ros2_medkit_gateway/belongs-to` |
| `golden/app_depends-on.json` | `GET /apps/ros2_medkit_gateway/depends-on` |
| `golden/functions_list.json` | `GET /functions` |
| `golden/function_get.json` | `GET /functions/root` |
| `golden/function_hosts.json` | `GET /functions/root/hosts` |
| `golden/collection_envelope_example.json` | `GET /components/spark-6723/hosts` — canonical full collection envelope (`items` + `_links` + `x-medkit.total_count`) for #80 to diff envelope shape against |
| `golden/faults_list.json` | `GET /faults` (default = CONFIRMED) |
| `golden/faults_list_all.json` | `GET /faults?status=all` |
| `golden/faults_filtered_pending.json` | `GET /faults?status=pending` (filtered — only the PREFAILED/pending fault) |
| `golden/component_faults_list.json` | `GET /components/spark-6723/faults` (component-level aggregation across owned apps) |
| `golden/app_faults_list.json` | `GET /apps/ros2_medkit_gateway/faults` (app-scoped) |
| `golden/fault_get_with_freezeframe.json` | `GET /apps/ros2_medkit_gateway/faults/BRAKE_PRESSURE_LOW` — **the DTC detail**: `item.status` sub-object (`aggregatedStatus`/`testFailed`/`confirmedDTC`/`pendingDTC`) + `environment_data.snapshots[]` freeze-frame + `extended_data_records` |
| `golden/error_not_found.json` | `GET /apps/does-not-exist` (GenericError envelope: `error_code`/`message`/`parameters`) |
| `golden/faults_stream_sse_sample.txt` | `GET /faults/stream` (raw SSE wire: `id:` / `event:` / `data:` frames) |
| `golden/faults_stream_event.json` | the `data:` payload of one SSE frame, pretty-printed |

### Envelope / casing landmarks worth knowing (the reason this corpus exists)

- Collection envelope: `{ "items": [...], "x-medkit": { "total_count": N }, "_links": {...} }`.
  Top-level lists (`/apps`, `/components`) carry `items` + `x-medkit.total_count`
  but **no** `_links`; relationship sub-resources add `_links`. Fault lists put the
  count under `x-medkit.count` (not `total_count`) plus `muted_count`/`cluster_count`.
- **Mixed casing is real**: the fault *list* item uses snake_case
  (`fault_code`, `occurrence_count`, `reporting_sources`, `severity_label`), while
  the fault *detail* (`fault_get_with_freezeframe.json`) renames to `item.code` /
  `item.fault_name` and nests a **camelCase** status object
  (`aggregatedStatus`, `testFailed`, `confirmedDTC`, `pendingDTC`) with string
  `"1"`/`"0"` values. Per-entity `x-medkit` extensions carry `status_raw`,
  `occurrence_count`, `reporting_sources`.

## RESOLVED: the `/auth/*` token-endpoint path

The published docs were reported (issue #79) as self-contradictory: the REST
reference page (`api/rest.html`) listing `GET/POST /api/v1/auth/tokens` (plural)
versus the auth tutorial listing `/auth/authorize` + `/token` + `/revoke`.

**Pinned from the running binary** (`GET /api/v1/docs` → `openapi.json`, and the
`GET /` endpoint catalogue in `root.json`):

| Path | Method | OpenAPI `operationId` / summary |
| --- | --- | --- |
| `/api/v1/auth/authorize` | `POST` | `authorizeClient` — "Authorize client" |
| `/api/v1/auth/token` | `POST` | `getToken` — "Obtain access token" (singular **token**) |
| `/api/v1/auth/revoke` | `POST` | `revokeToken` — "Revoke token" |

- The token endpoint is **`POST /api/v1/auth/token`** (singular). There is **no**
  `/auth/tokens` path and **no** `GET` on any `/auth/*` route in this build.
- Therefore the **`api/rest.html` `/auth/tokens` (plural, with `GET`) form is the
  WRONG one**; the authentication-tutorial form (`authorize` / `token` / `revoke`,
  POST-only) is correct and matches the binary.
- Cross-check: in the reference checkout at commit `243a2f4a`, **both** doc sources
  (`docs/api/rest.rst` and `docs/tutorials/authentication.rst`) already agree on
  `authorize` / `token` / `revoke` — so the `/auth/tokens` discrepancy lives in the
  **published HTML site** (an older/stale render), not in the source at this commit.
- These routes are only **registered when authentication is enabled**; the demo runs
  with auth disabled, so a live `POST /api/v1/auth/token` returns `404` here even
  though the binary's own OpenAPI declares the path. The OpenAPI declaration is the
  authoritative contract.

## Mock `fault_manager` used for the fault captures (verbatim)

```python
#!/usr/bin/env python3
"""Minimal ROS 2 fault_manager mock to drive the ros2_medkit gateway's REAL
fault handlers so we can capture authentic REST response bodies (DTC status
sub-object + freeze-frame) from the running C++ binary."""
import rclpy
from rclpy.node import Node
from builtin_interfaces.msg import Time
from ros2_medkit_msgs.srv import ListFaults, GetFault, ReportFault, ClearFault
from ros2_medkit_msgs.msg import Fault, FaultEvent, EnvironmentData, Snapshot, ExtendedDataRecords


def t(sec, nsec=0):
    m = Time(); m.sec = sec; m.nanosec = nsec; return m


def confirmed_fault():
    f = Fault()
    f.fault_code = "BRAKE_PRESSURE_LOW"
    f.severity = Fault.SEVERITY_ERROR
    f.description = "Brake circuit pressure below safe threshold"
    f.first_occurred = t(1782600000, 250000000)
    f.last_occurred = t(1782661500, 750000000)
    f.occurrence_count = 7
    f.status = Fault.STATUS_CONFIRMED
    f.reporting_sources = ["/ros2_medkit_gateway"]
    return f


def prefailed_fault():
    f = Fault()
    f.fault_code = "MOTOR_OVERHEAT"
    f.severity = Fault.SEVERITY_WARN
    f.description = "Drive motor temperature trending high"
    f.first_occurred = t(1782660000, 0)
    f.last_occurred = t(1782661400, 0)
    f.occurrence_count = 2
    f.status = Fault.STATUS_PREFAILED
    f.reporting_sources = ["/ros2_medkit_gateway_sub"]
    return f


def environment_data():
    env = EnvironmentData()
    snap = Snapshot()
    snap.type = Snapshot.TYPE_FREEZE_FRAME
    snap.name = "freeze_frame_at_confirmation"
    snap.data = '{"vehicle_speed_kph": 42.5, "brake_pressure_bar": 11.3, "ambient_temp_c": 31.0}'
    snap.topic = "/diagnostics/brake_state"
    snap.message_type = "diagnostic_msgs/msg/DiagnosticStatus"
    snap.captured_at_ns = 1782661500750000000
    env.snapshots = [snap]
    edr = ExtendedDataRecords()
    edr.first_occurrence_ns = 1782600000250000000
    edr.last_occurrence_ns = 1782661500750000000
    env.extended_data_records = edr
    return env


class FaultManagerMock(Node):
    def __init__(self):
        super().__init__("fault_manager")
        base = "/fault_manager"
        self.create_service(ListFaults, base + "/list_faults", self.on_list)
        self.create_service(GetFault, base + "/get_fault", self.on_get)
        self.create_service(ReportFault, base + "/report_fault", self.on_report)
        self.create_service(ClearFault, base + "/clear_fault", self.on_clear)
        self.pub = self.create_publisher(FaultEvent, base + "/events", 10)
        self.timer = self.create_timer(1.0, self.tick)

    def on_list(self, req, resp):
        wants = list(req.statuses) if req.statuses else ["CONFIRMED"]
        out = []
        if "CONFIRMED" in wants: out.append(confirmed_fault())
        if "PREFAILED" in wants: out.append(prefailed_fault())
        resp.faults = out; resp.muted_count = 0; resp.cluster_count = 0
        return resp

    def on_get(self, req, resp):
        if req.fault_code == "BRAKE_PRESSURE_LOW":
            resp.success = True; resp.fault = confirmed_fault(); resp.environment_data = environment_data()
        else:
            resp.success = False; resp.error_message = "Fault not found"
        return resp

    def on_report(self, req, resp):
        resp.accepted = True; return resp

    def on_clear(self, req, resp):
        resp.success = True; resp.message = "cleared"; return resp

    def tick(self):
        ev = FaultEvent(); ev.event_type = FaultEvent.EVENT_CONFIRMED
        ev.fault = confirmed_fault(); ev.timestamp = t(1782661500, 750000000)
        self.pub.publish(ev)


def main():
    rclpy.init(); rclpy.spin(FaultManagerMock())


if __name__ == "__main__":
    main()
```
