## evidence_id

ev_5d0e1d9ec222d827372cebd627eee2e69ed310d097693ce2d4c46efbfcf74b9b

## subject.name

soothe3.bin

## subject.digest

| Key    | Value                                                            |
|--------|------------------------------------------------------------------|
| sha256 | 0143f03b7c7f1e41a51896f269a64c3dfc4c804e94ce8cccbeef9c98dbbcfa64 |

## subject.local_path

/tmp/opencode/rea/soothe3.bin

## subject.format

pe

## subject.architecture

x86_64

## provider

| Key     | Value  |
|---------|--------|
| id      | ghidra |
| name    | Ghidra |
| version | 12.1.4 |

## analysis_profile.provider

| Key     | Value  |
|---------|--------|
| id      | ghidra |
| name    | Ghidra |
| version | 12.1.4 |

## analysis_profile.parameters.target_kind

executable

## analysis_profile.parameters.target_format

pe

## analysis_profile.parameters.executable_role

shared-library

## analysis_profile.parameters.managed

false

## analysis_profile.parameters.native_aot_metadata

| Key                  | Value                       |
|----------------------|-----------------------------|
| contract_revision    | rtr-9.1-x64-pe-read-only-v5 |
| source_bytes         | 134217728                   |
| work_units           | 67108864                    |
| working_memory_bytes | 67108864                    |
| report_bytes         | 4194304                     |

## analysis_profile.parameters.architecture

x86_64

## analysis_profile.parameters.available_architectures

x86_64

## analysis_profile.parameters.import_mode

ephemeral-source-immutable

## analysis_profile.parameters.pdb_observations

program-info-v5

## analysis_profile.parameters.annotation_policy

atomic-function-entry-metadata-v1

## analysis_profile.parameters.load_image_observations

source-mappings-entry-context-v2

## analysis_profile.parameters.function_body_evidence

complete-inclusive-ranges-v1

## analysis_profile.parameters.function_references

complete-body-and-entry-reference-manager-v2

## analysis_profile.parameters.call_relationship_coverage

function-body-call-coverage-v5

## analysis_profile.parameters.function_dossier_coverage

observed-facets-and-call-relationships-v1

## analysis_profile.parameters.string_inventory_evidence

defined-data-coverage-v1

## analysis_profile.parameters.location_resolution

explicit-address-exact-entry-symbol-first-v3

## analysis_profile.parameters.procedure_file_offset_resolution

address-first-unique-target-source-mapping-v2

## analysis_profile.parameters.instruction_flow_evidence

decoded-return-pcode-v1

## analysis_profile.parameters.instruction_references

exact-address-incoming-without-synthetic-entry-v2

## analysis_profile.parameters.function_boundary_observations

ghidra-function-boundary-limitations-v4

## analysis_profile.parameters.no_return_repair

returning-imports-decoded-return-v2

## analysis_profile.parameters.process_launch

inspected-jvm-launch-support-v1

## analysis_profile.parameters.jump_table_evidence

typed-case-default-blocks-v1

## analysis_profile.parameters.decompiler_jump_loads

true

## analysis_profile.parameters.loader

auto-from-header

## analysis_profile.parameters.language_id

auto-from-header

## analysis_profile.parameters.compiler_spec_id

auto-default

## analysis_profile.parameters.analyzer_preset

ghidra-default

## analysis_profile.digest

23500ca49a1d1c30ade9499cc7ce87c5376277a0bb16225ff60f763f2dcfb2e2

## predicate_type

rea.analysis

## operation

procedure_pseudo_code

## parameters

| Key       | Value       |
|-----------|-------------|
| procedure | 0x1808f78e0 |

## raw_result.value


void FUN_1808f78e0(longlong *param_1,longlong param_2,int *param_3)

{
  int iVar1;
  int iVar2;
  longlong lVar3;
  undefined4 uVar4;
  undefined4 uVar5;
  longlong lVar6;
  longlong lVar7;
  longlong lVar8;
  longlong *plVar9;
  longlong *plVar10;
  undefined8 *puVar11;
  longlong *plVar12;
  ulonglong uVar13;
  int iVar14;
  longlong *plVar15;
  longlong lVar16;
  int *piVar17;
  longlong local_508;
  undefined8 local_4e0 [16];
  uint local_460;
  int local_45c;
  longlong local_458;
  longlong local_448 [16];
  uint local_3c8;
  int iStack_3c4;
  longlong lStack_3c0;
  uint local_48;
  undefined4 local_44;
  
  lVar16 = 8;
  plVar12 = param_1;
  plVar9 = local_448;
  do {
    plVar15 = plVar9;
    plVar10 = plVar12;
    lVar8 = plVar10[1];
    lVar3 = plVar10[2];
    lVar6 = plVar10[3];
    *plVar15 = *plVar10;
    plVar15[1] = lVar8;
    lVar8 = plVar10[4];
    lVar7 = plVar10[5];
    plVar15[2] = lVar3;
    plVar15[3] = lVar6;
    lVar3 = plVar10[6];
    lVar6 = plVar10[7];
    plVar15[4] = lVar8;
    plVar15[5] = lVar7;
    lVar8 = plVar10[8];
    lVar7 = plVar10[9];
    plVar15[6] = lVar3;
    plVar15[7] = lVar6;
    lVar3 = plVar10[10];
    lVar6 = plVar10[0xb];
    plVar15[8] = lVar8;
    plVar15[9] = lVar7;
    lVar8 = plVar10[0xc];
    lVar7 = plVar10[0xd];
    plVar15[10] = lVar3;
    plVar15[0xb] = lVar6;
    lVar3 = plVar10[0xe];
    lVar6 = plVar10[0xf];
    plVar15[0xc] = lVar8;
    plVar15[0xd] = lVar7;
    plVar15[0xe] = lVar3;
    plVar15[0xf] = lVar6;
    lVar16 = lVar16 + -1;
    plVar12 = plVar10 + 0x10;
    plVar9 = plVar15 + 0x10;
  } while (lVar16 != 0);
  uVar4 = *(undefined4 *)((longlong)plVar10 + 0x84);
  lVar16 = plVar10[0x11];
  uVar5 = *(undefined4 *)((longlong)plVar10 + 0x8c);
  *(int *)(plVar15 + 0x10) = (int)plVar10[0x10];
  *(undefined4 *)((longlong)plVar15 + 0x84) = uVar4;
  *(int *)(plVar15 + 0x11) = (int)lVar16;
  *(undefined4 *)((longlong)plVar15 + 0x8c) = uVar5;
  if (0 < (int)local_48) {
    plVar12 = local_448;
    uVar13 = (ulonglong)local_48;
    do {
      thunk_FUN_181dae2a0(*plVar12,local_44);
      plVar12 = plVar12 + 1;
      uVar13 = uVar13 - 1;
    } while (uVar13 != 0);
  }
  lVar16 = 0;
  if (0 < *(int *)(param_2 + 0x404)) {
    lVar8 = param_1[0x80];
    piVar17 = param_3;
    do {
      iVar1 = piVar17[0x20];
      iVar2 = *piVar17;
      if (0 < (longlong)(int)lVar8) {
        plVar12 = param_1;
        local_508 = (longlong)(int)lVar8;
        do {
          iVar14 = 0;
          local_460 = param_3[100];
          lVar3 = *(longlong *)((param_2 - (longlong)param_1) + (longlong)plVar12);
          if (local_460 == 0) {
            local_460 = param_3[0x60];
          }
          local_45c = param_3[0x62];
          local_4e0[0] = 0;
          local_4e0[0xf] = 0;
          local_458 = (longlong)param_3[0x62] << 2;
          local_4e0[1] = 0;
          local_4e0[2] = 0;
          local_4e0[3] = 0;
          local_4e0[4] = 0;
          local_4e0[5] = 0;
          local_4e0[6] = 0;
          local_4e0[7] = 0;
          local_4e0[8] = 0;
          local_4e0[9] = 0;
          local_4e0[10] = 0;
          local_4e0[0xb] = 0;
          local_4e0[0xc] = 0;
          local_4e0[0xd] = 0;
          local_4e0[0xe] = 0;
          if (0 < (int)local_460) {
            puVar11 = local_4e0;
            do {
              iVar14 = iVar14 + 1;
              *puVar11 = *(undefined8 *)
                          ((longlong)param_3 + (0x100 - (longlong)local_4e0) + (longlong)puVar11);
              puVar11 = puVar11 + 1;
            } while (iVar14 < (int)local_460);
          }
          iStack_3c4 = (int)*(undefined8 *)(param_3 + 0x66);
          lStack_3c0 = (longlong)iStack_3c4 << 2;
          local_448[0] = 0;
          local_448[0xf] = 0;
          local_448[1] = 0;
          local_448[2] = 0;
          local_448[3] = 0;
          local_448[4] = 0;
          local_448[5] = 0;
          local_448[6] = 0;
          local_448[7] = 0;
          local_448[8] = 0;
          local_448[9] = 0;
          local_448[10] = 0;
          local_448[0xb] = 0;
          local_448[0xc] = 0;
          local_448[0xd] = 0;
          local_448[0xe] = 0;
          local_3c8 = local_460;
          if ((0 < (int)local_460) && ((1 < local_460 || (0 < (int)local_460)))) {
            FUN_1817bb8d6(local_448,local_4e0,(longlong)(int)local_460 << 3);
          }
          thunk_FUN_181dae2a0(*(longlong *)
                               (((longlong)local_448 - (longlong)param_3) + (longlong)piVar17) +
                              (longlong)iVar2 * 4,*(undefined4 *)(lVar3 + lVar16 * 4),
                              (longlong)iVar2 * 4 + *plVar12,iVar1);
          plVar12 = plVar12 + 1;
          local_508 = local_508 + -1;
        } while (local_508 != 0);
      }
      lVar16 = lVar16 + 1;
      piVar17 = piVar17 + 2;
    } while (lVar16 < *(int *)(param_2 + 0x404));
  }
  return;
}



## raw_result.limitations



## normalized_result


void FUN_1808f78e0(longlong *param_1,longlong param_2,int *param_3)

{
  int iVar1;
  int iVar2;
  longlong lVar3;
  undefined4 uVar4;
  undefined4 uVar5;
  longlong lVar6;
  longlong lVar7;
  longlong lVar8;
  longlong *plVar9;
  longlong *plVar10;
  undefined8 *puVar11;
  longlong *plVar12;
  ulonglong uVar13;
  int iVar14;
  longlong *plVar15;
  longlong lVar16;
  int *piVar17;
  longlong local_508;
  undefined8 local_4e0 [16];
  uint local_460;
  int local_45c;
  longlong local_458;
  longlong local_448 [16];
  uint local_3c8;
  int iStack_3c4;
  longlong lStack_3c0;
  uint local_48;
  undefined4 local_44;
  
  lVar16 = 8;
  plVar12 = param_1;
  plVar9 = local_448;
  do {
    plVar15 = plVar9;
    plVar10 = plVar12;
    lVar8 = plVar10[1];
    lVar3 = plVar10[2];
    lVar6 = plVar10[3];
    *plVar15 = *plVar10;
    plVar15[1] = lVar8;
    lVar8 = plVar10[4];
    lVar7 = plVar10[5];
    plVar15[2] = lVar3;
    plVar15[3] = lVar6;
    lVar3 = plVar10[6];
    lVar6 = plVar10[7];
    plVar15[4] = lVar8;
    plVar15[5] = lVar7;
    lVar8 = plVar10[8];
    lVar7 = plVar10[9];
    plVar15[6] = lVar3;
    plVar15[7] = lVar6;
    lVar3 = plVar10[10];
    lVar6 = plVar10[0xb];
    plVar15[8] = lVar8;
    plVar15[9] = lVar7;
    lVar8 = plVar10[0xc];
    lVar7 = plVar10[0xd];
    plVar15[10] = lVar3;
    plVar15[0xb] = lVar6;
    lVar3 = plVar10[0xe];
    lVar6 = plVar10[0xf];
    plVar15[0xc] = lVar8;
    plVar15[0xd] = lVar7;
    plVar15[0xe] = lVar3;
    plVar15[0xf] = lVar6;
    lVar16 = lVar16 + -1;
    plVar12 = plVar10 + 0x10;
    plVar9 = plVar15 + 0x10;
  } while (lVar16 != 0);
  uVar4 = *(undefined4 *)((longlong)plVar10 + 0x84);
  lVar16 = plVar10[0x11];
  uVar5 = *(undefined4 *)((longlong)plVar10 + 0x8c);
  *(int *)(plVar15 + 0x10) = (int)plVar10[0x10];
  *(undefined4 *)((longlong)plVar15 + 0x84) = uVar4;
  *(int *)(plVar15 + 0x11) = (int)lVar16;
  *(undefined4 *)((longlong)plVar15 + 0x8c) = uVar5;
  if (0 < (int)local_48) {
    plVar12 = local_448;
    uVar13 = (ulonglong)local_48;
    do {
      thunk_FUN_181dae2a0(*plVar12,local_44);
      plVar12 = plVar12 + 1;
      uVar13 = uVar13 - 1;
    } while (uVar13 != 0);
  }
  lVar16 = 0;
  if (0 < *(int *)(param_2 + 0x404)) {
    lVar8 = param_1[0x80];
    piVar17 = param_3;
    do {
      iVar1 = piVar17[0x20];
      iVar2 = *piVar17;
      if (0 < (longlong)(int)lVar8) {
        plVar12 = param_1;
        local_508 = (longlong)(int)lVar8;
        do {
          iVar14 = 0;
          local_460 = param_3[100];
          lVar3 = *(longlong *)((param_2 - (longlong)param_1) + (longlong)plVar12);
          if (local_460 == 0) {
            local_460 = param_3[0x60];
          }
          local_45c = param_3[0x62];
          local_4e0[0] = 0;
          local_4e0[0xf] = 0;
          local_458 = (longlong)param_3[0x62] << 2;
          local_4e0[1] = 0;
          local_4e0[2] = 0;
          local_4e0[3] = 0;
          local_4e0[4] = 0;
          local_4e0[5] = 0;
          local_4e0[6] = 0;
          local_4e0[7] = 0;
          local_4e0[8] = 0;
          local_4e0[9] = 0;
          local_4e0[10] = 0;
          local_4e0[0xb] = 0;
          local_4e0[0xc] = 0;
          local_4e0[0xd] = 0;
          local_4e0[0xe] = 0;
          if (0 < (int)local_460) {
            puVar11 = local_4e0;
            do {
              iVar14 = iVar14 + 1;
              *puVar11 = *(undefined8 *)
                          ((longlong)param_3 + (0x100 - (longlong)local_4e0) + (longlong)puVar11);
              puVar11 = puVar11 + 1;
            } while (iVar14 < (int)local_460);
          }
          iStack_3c4 = (int)*(undefined8 *)(param_3 + 0x66);
          lStack_3c0 = (longlong)iStack_3c4 << 2;
          local_448[0] = 0;
          local_448[0xf] = 0;
          local_448[1] = 0;
          local_448[2] = 0;
          local_448[3] = 0;
          local_448[4] = 0;
          local_448[5] = 0;
          local_448[6] = 0;
          local_448[7] = 0;
          local_448[8] = 0;
          local_448[9] = 0;
          local_448[10] = 0;
          local_448[0xb] = 0;
          local_448[0xc] = 0;
          local_448[0xd] = 0;
          local_448[0xe] = 0;
          local_3c8 = local_460;
          if ((0 < (int)local_460) && ((1 < local_460 || (0 < (int)local_460)))) {
            FUN_1817bb8d6(local_448,local_4e0,(longlong)(int)local_460 << 3);
          }
          thunk_FUN_181dae2a0(*(longlong *)
                               (((longlong)local_448 - (longlong)param_3) + (longlong)piVar17) +
                              (longlong)iVar2 * 4,*(undefined4 *)(lVar3 + lVar16 * 4),
                              (longlong)iVar2 * 4 + *plVar12,iVar1);
          plVar12 = plVar12 + 1;
          local_508 = local_508 + -1;
        } while (local_508 != 0);
      }
      lVar16 = lVar16 + 1;
      piVar17 = piVar17 + 2;
    } while (lVar16 < *(int *)(param_2 + 0x404));
  }
  return;
}



## confidence

observed

## authority

shipped-artifact

## environment

null

## limitations

The session serves operations only after Ghidra reports default auto-analysis complete; incomplete analysis does not expose partial results.,The imported Program and temporary project are ephemeral and deleted on close. Annotation operations edit session database metadata; original executable bytes are never written.,Cancelling an active Ghidra request terminates its ephemeral database and discards session annotations. The selected target remains open; the next provider query imports the original artifact again.,Default-space addresses use lowercase 0x-prefixed hexadecimal; other address spaces use <percent-encoded-space>:0x<hex>.,Pseudocode is Ghidra decompiler output, not original source and not text-equivalent to Hopper output.,External functions and functions without an analyzable body return null; other decompiler failures remain explicit.

## relationship_coverage

null

## locations



## evidence_links

