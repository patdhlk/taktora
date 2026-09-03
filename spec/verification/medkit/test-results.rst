Medkit test-execution results
=============================

Executed-test results for the medkit crates, ingested from the
``cargo-nextest`` JUnit file that the ``ci-test-records`` workflow produces
(:need:`REQ_1011`). Each ingested case is a ``test-case`` need carrying its
``result`` and ``time``; the build joins them to the :doc:`verification cases
<index>` each test declares with a ``@need-ids:`` marker (:need:`REQ_1013`)
and emits ``test-execution-record.json`` next to this page (:need:`REQ_1014`,
:need:`REQ_1015`).

The results below are regenerated on every code-changing CI run and are
**not** committed: a plain documentation build has no JUnit to ingest and
renders a note here instead. The record itself is the validation evidence for
the commit it names; this page is its human-readable companion.

.. test-results-if-present:: Medkit nextest results
   :id: TF_MEDKIT
   :file: ../target/nextest/ci/junit.xml
   :auto_suites:
   :auto_cases:
