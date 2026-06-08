CLI and vendoring tests
=======================

Test cases for the CLI and ESI vendoring surface (:need:`FEAT_0084`).
Live under ``crates/ethercat-netcfg-cli/tests/``.

.. test:: expand subcommand prints the build-equivalent module
   :id: TEST_0841
   :status: open
   :verifies: REQ_0832

   ``netcfg expand`` over a fixture prints to stdout a module
   byte-identical to the one the build helper writes to ``OUT_DIR`` for
   the same input.

.. test:: fetch vendors a remote ESI and records a pinned lockfile
   :id: TEST_0842
   :status: open
   :verifies: REQ_0833, REQ_0835

   Against a local HTTP test server, ``netcfg fetch`` downloads a
   referenced ESI into the vendored directory and writes a lockfile entry
   carrying the file's content hash and the device revision. A second
   ``fetch`` with the server returning identical bytes is a no-op; a
   server returning altered bytes updates the recorded hash.

.. test:: Build fetches nothing; unvendored URL is a build error
   :id: TEST_0843
   :status: open
   :verifies: REQ_0834

   With network access denied to the build process, a fixture whose
   device references a web-URL ESI that has no vendored, pinned local
   copy fails the build with a "not vendored" error and makes no network
   request. The same fixture after ``netcfg fetch`` builds cleanly
   offline.
