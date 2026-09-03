Workspace tooling
=================

Repo-wide developer-tooling requirements — infrastructure that spans all
workspace crates rather than any one subsystem. Residents: test-coverage
measurement (:need:`FEAT_0120`), the onboarding golden path
(:need:`FEAT_0121`) and test-execution records (:need:`FEAT_0122`).
Candidates for later migration: the complexity gate, the publish-ordering
guards.

.. toctree::
   :maxdepth: 2

   coverage
   onboarding
   test-records
