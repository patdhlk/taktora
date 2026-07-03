Workspace tooling
=================

Repo-wide developer-tooling requirements — infrastructure that spans all
workspace crates rather than any one subsystem. First resident: test-coverage
measurement (:need:`FEAT_0120`). Candidates for later migration: the
complexity gate, the publish-ordering guards.

.. toctree::
   :maxdepth: 2

   coverage
