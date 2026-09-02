Test-execution records
======================

Standing tooling that binds a *build* to the *tests that validated it*: which
tests ran, whether they passed, which :need:`TEST_0900`-style verification
cases they exercised, and — through those cases — which requirements they
validate. The record is a machine-readable artifact stamped with the build
identity (:need:`REQ_0990`), so a field issue can be traced from a deployed
commit back to the validation evidence for that commit.

.. feat:: Test-execution records
   :id: FEAT_0122
   :status: implemented

   **Motivation.** Build identity (:need:`REQ_0990`) tells a deployed binary
   *which commit* it is. It does not tell anyone *whether that commit's tests
   passed* or *which requirements those tests validate*. The spec already
   carries verification cases (``test`` needs) that ``:verifies:`` their
   requirements, but nothing records that those cases actually executed and
   passed against a given build — that fact lives in a green CI badge, not in
   a traceable artifact. For a build→field traceability argument, the
   validation evidence for a commit must be a durable, machine-readable
   record, not folklore.

   **Scope (medkit pilot).** A CI-produced ``test-execution-record.json`` for
   the medkit verification subtree, assembled from native sphinx-needs data:
   the medkit crates' tests run under ``cargo-nextest`` (JUnit output); the
   results are ingested as ``test-file`` / ``test-case`` needs
   (``sphinx-test-reports``); each test declares the verification case(s) it
   exercises with a ``@need-ids:`` source marker extracted by
   ``sphinx-codelinks`` and build-enforced by the spec's own join extension
   (a dangling marker fails ``sphinx-build -W``); a build-time join links each executed
   ``test-case`` to the ``test`` need it validates; and the build emits a
   record — stamped with the build identity — projecting, per commit, the
   executed tests, their results, the verification cases exercised, and the
   requirements thereby validated, plus an honest list of medkit verification
   cases with no passing test behind them.

   **Non-goals.** Migrating the whole workspace's ``//! TEST_`` prose tags to
   ``@need-ids:`` markers (a later slice — this pilot proves the pipeline on
   medkit only); baking the record into the runtime binary (it is a
   CI/release artifact, queried offline, not a runtime diagnostics surface —
   contrast :need:`REQ_0990`, which does travel in the binary); the later
   production-traceability slices (release-as-artifact, deployment/device
   mapping, SBOM, approval/sign-off).

.. req:: Structured medkit test results via cargo-nextest
   :id: REQ_1010
   :status: implemented
   :satisfies: FEAT_0122
   :links: IMPL_0093, TEST_0981

   The medkit crates' tests shall execute under ``cargo-nextest`` producing a
   JUnit XML result file, so each test's pass/fail outcome and duration is
   machine-readable. ``cargo-nextest`` runs on the pinned stable toolchain and
   emits JUnit natively (:need:`ADR_0139`); plain ``cargo test`` — used
   elsewhere in CI — produces no structured output and cannot feed the record.

.. req:: Execution results ingested as needs
   :id: REQ_1011
   :status: implemented
   :satisfies: FEAT_0122
   :links: IMPL_0093, TEST_0982

   The spec build shall ingest the medkit JUnit results with
   ``sphinx-test-reports``, creating ``test-file`` and ``test-case`` needs
   that carry each test's ``result`` (passed / failed / skipped) and ``time``.
   Execution outcomes thereby become first-class sphinx-needs objects,
   queryable and linkable like any other need, rather than an opaque XML blob.

.. req:: Source-to-case binding via markers, build-enforced
   :id: REQ_1012
   :status: implemented
   :satisfies: FEAT_0122
   :links: IMPL_0093, TEST_0983

   Each medkit test shall declare the verification case(s) it exercises with a
   ``@need-ids:`` source-comment marker (for example
   ``// @need-ids: TEST_0904``), extracted by ``sphinx-codelinks``. A marker
   naming a verification-case id that does not resolve to an existing ``test``
   need shall fail the strict build (``sphinx-build -W``), so the binding
   between a test and the case it claims to satisfy is machine-checked at the
   source, not asserted in prose. These markers replace the medkit crates'
   existing ``//! TEST_`` documentation tags.

.. req:: Executed-test to verification-case join
   :id: REQ_1013
   :status: implemented
   :satisfies: FEAT_0122
   :links: IMPL_0093, TEST_0984

   The build shall link each ingested ``test-case`` need to the ``test`` need
   it validates, joining the ``sphinx-codelinks`` marker map to the
   ``sphinx-test-reports`` results on test-function identity (crate plus test
   name). A passing ``test-case`` is thereby connected to the verification
   case it exercises, and — because a ``test`` need already ``:verifies:`` its
   requirements — to the requirements it validates. The join shall be data
   over the two extensions' outputs, adding no bespoke record-builder.

.. req:: Build-identity-stamped record projection
   :id: REQ_1014
   :status: implemented
   :satisfies: FEAT_0122
   :links: IMPL_0093, TEST_0985

   The build shall emit ``test-execution-record.json`` stamped with the build
   identity — the same git commit fields defined for :need:`REQ_0990` (full
   and short hash, ``git describe``, dirty flag, build timestamp) — sourced
   from the CI checkout, so the record and a deployed binary correlate by
   commit. The record shall project, for that commit, the executed medkit
   tests, their results, the verification cases exercised, and the
   requirements thereby validated.

.. req:: Coverage-gap enumeration
   :id: REQ_1015
   :status: implemented
   :satisfies: FEAT_0122
   :links: IMPL_0093, TEST_0985

   The record shall enumerate the medkit verification cases (``test`` needs)
   that no passing ``test-case`` is joined to — whether because no test
   carries the marker or because the joined test failed — so partial pilot
   coverage is reported, never silently omitted. A record that listed only
   validated cases would read as complete when it is not.

.. req:: CI regenerates and publishes the record
   :id: REQ_1016
   :status: implemented
   :satisfies: FEAT_0122
   :links: IMPL_0093, TEST_0986

   CI shall regenerate ``test-execution-record.json`` on code-changing pull
   requests and pushes to ``main`` — running the medkit ``cargo-nextest`` leg
   and the spec build that assembles the record — and publish it as a workflow
   artifact. The record is the release-facing home for the validation evidence
   (:need:`FEAT_0122`); it is reproducible from any commit, so it is published,
   not committed.

Requirements at a glance
------------------------

.. needtable::
   :columns: id, title, status, links
   :show_filters:
   :filter: "FEAT_0122" in satisfies
