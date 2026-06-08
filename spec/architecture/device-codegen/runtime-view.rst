Runtime view
============

arc42 §6 — the two sequences that matter for a build-time toolchain: the
codegen run inside a consumer's ``build.rs``, and the preop bring-up
sequence a generated device's ``configure`` performs on the bus.

.. architecture:: Build-time generation flow
   :id: ARCH_0052
   :status: open
   :refines: FEAT_0055

   The build-time codegen sequence when a downstream crate's
   ``build.rs`` runs.

   .. mermaid::

      sequenceDiagram
        participant Cargo as cargo
        participant Build as build.rs
        participant Esi as ethercat-esi
        participant Codegen as ethercat-esi-codegen
        participant Backend as -codegen-ethercrab
        participant PP as prettyplease
        participant Out as $OUT_DIR/devices.rs

        Cargo->>Build: invoke build.rs
        Build->>Esi: parse(xml) for each ESI file
        Esi-->>Build: EsiFile IR
        Build->>Codegen: generate(IR, backend)
        Codegen->>Backend: emit_device(d) per device
        Backend-->>Codegen: TokenStream
        Codegen->>Backend: emit_module_root(devices)
        Backend-->>Codegen: TokenStream (with registry!())
        Codegen-->>Build: combined TokenStream
        Build->>PP: unparse(tokenstream)
        PP-->>Build: formatted source
        Build->>Out: write devices.rs
        Build-->>Cargo: cargo:rerun-if-changed=esi/*.xml

.. architecture:: Preop bring-up flow (per device)
   :id: ARCH_0053
   :status: open
   :refines: FEAT_0054

   The runtime sequence when a generated device's ``configure``
   method runs during the EtherCAT bus's PRE-OP → SAFE-OP
   transition (per :need:`REQ_0315`).

   .. mermaid::

      sequenceDiagram
        participant App as application
        participant Dev as <Device>
        participant Sub as SubDevicePreOperational
        participant Bus as EtherCAT bus

        App->>Dev: Device::default()
        App->>Dev: configure(&sub, Assignment::Standard).await
        Dev->>Sub: sdo_write(0x1C12, 0, 0) (clear)
        Sub->>Bus: SDO download
        loop per RxPDO in alternative
          Dev->>Sub: sdo_write(0x1C12, idx, pdo_index)
          Sub->>Bus: SDO download
        end
        Dev->>Sub: sdo_write(0x1C12, 0, N) (commit count)
        Dev->>Sub: sdo_write(0x1C13, 0, 0)
        loop per TxPDO in alternative
          Dev->>Sub: sdo_write(0x1C13, idx, pdo_index)
        end
        Dev->>Sub: sdo_write(0x1C13, 0, M)
        loop per InitCmd in ESI mailbox section
          Dev->>Sub: sdo_write(initcmd.index, initcmd.subindex, initcmd.data)
        end
        Dev-->>App: Ok(())
        Note over Sub,Bus: caller transitions PRE-OP → SAFE-OP
