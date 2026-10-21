## evidence_id

ev_d0ce8e3f6f392d87769204fd43032ebce54d00bffa485e75e90821c97cd80387

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
| procedure | 0x1808f03f0 |

## raw_result.value


void FUN_1808f03f0(longlong param_1,undefined8 *param_2,undefined8 *param_3)

{
  undefined4 uVar1;
  undefined4 uVar2;
  undefined4 uVar3;
  undefined4 uVar4;
  undefined8 uVar5;
  undefined8 uVar6;
  undefined8 uVar7;
  undefined8 uVar8;
  undefined8 *puVar9;
  undefined8 *puVar10;
  undefined8 *puVar11;
  longlong lVar12;
  longlong lVar13;
  undefined8 local_828 [130];
  undefined8 local_418 [131];
  
  lVar12 = 8;
  lVar13 = 8;
  puVar9 = local_828;
  do {
    puVar11 = param_3;
    puVar10 = puVar9;
    uVar5 = puVar11[1];
    uVar6 = puVar11[2];
    uVar7 = puVar11[3];
    *puVar10 = *puVar11;
    puVar10[1] = uVar5;
    uVar5 = puVar11[4];
    uVar8 = puVar11[5];
    puVar10[2] = uVar6;
    puVar10[3] = uVar7;
    uVar6 = puVar11[6];
    uVar7 = puVar11[7];
    puVar10[4] = uVar5;
    puVar10[5] = uVar8;
    uVar5 = puVar11[8];
    uVar8 = puVar11[9];
    puVar10[6] = uVar6;
    puVar10[7] = uVar7;
    uVar6 = puVar11[10];
    uVar7 = puVar11[0xb];
    puVar10[8] = uVar5;
    puVar10[9] = uVar8;
    uVar5 = puVar11[0xc];
    uVar8 = puVar11[0xd];
    puVar10[10] = uVar6;
    puVar10[0xb] = uVar7;
    uVar6 = puVar11[0xe];
    uVar7 = puVar11[0xf];
    puVar10[0xc] = uVar5;
    puVar10[0xd] = uVar8;
    puVar10[0xe] = uVar6;
    puVar10[0xf] = uVar7;
    lVar13 = lVar13 + -1;
    puVar9 = puVar10 + 0x10;
    param_3 = puVar11 + 0x10;
  } while (lVar13 != 0);
  uVar5 = puVar11[0x11];
  puVar10[0x10] = puVar11[0x10];
  puVar10[0x11] = uVar5;
  puVar9 = local_418;
  do {
    puVar11 = param_2;
    puVar10 = puVar9;
    uVar5 = puVar11[1];
    uVar6 = puVar11[2];
    uVar7 = puVar11[3];
    *puVar10 = *puVar11;
    puVar10[1] = uVar5;
    uVar5 = puVar11[4];
    uVar8 = puVar11[5];
    puVar10[2] = uVar6;
    puVar10[3] = uVar7;
    uVar6 = puVar11[6];
    uVar7 = puVar11[7];
    puVar10[4] = uVar5;
    puVar10[5] = uVar8;
    uVar5 = puVar11[8];
    uVar8 = puVar11[9];
    puVar10[6] = uVar6;
    puVar10[7] = uVar7;
    uVar6 = puVar11[10];
    uVar7 = puVar11[0xb];
    puVar10[8] = uVar5;
    puVar10[9] = uVar8;
    uVar5 = puVar11[0xc];
    uVar8 = puVar11[0xd];
    puVar10[10] = uVar6;
    puVar10[0xb] = uVar7;
    uVar1 = *(undefined4 *)(puVar11 + 0xe);
    uVar2 = *(undefined4 *)((longlong)puVar11 + 0x74);
    uVar3 = *(undefined4 *)(puVar11 + 0xf);
    uVar4 = *(undefined4 *)((longlong)puVar11 + 0x7c);
    puVar10[0xc] = uVar5;
    puVar10[0xd] = uVar8;
    *(undefined4 *)(puVar10 + 0xe) = uVar1;
    *(undefined4 *)((longlong)puVar10 + 0x74) = uVar2;
    *(undefined4 *)(puVar10 + 0xf) = uVar3;
    *(undefined4 *)((longlong)puVar10 + 0x7c) = uVar4;
    lVar12 = lVar12 + -1;
    puVar9 = puVar10 + 0x10;
    param_2 = puVar11 + 0x10;
  } while (lVar12 != 0);
  uVar1 = *(undefined4 *)((longlong)puVar11 + 0x84);
  uVar2 = *(undefined4 *)(puVar11 + 0x11);
  uVar3 = *(undefined4 *)((longlong)puVar11 + 0x8c);
  *(undefined4 *)(puVar10 + 0x10) = *(undefined4 *)(puVar11 + 0x10);
  *(undefined4 *)((longlong)puVar10 + 0x84) = uVar1;
  *(undefined4 *)(puVar10 + 0x11) = uVar2;
  *(undefined4 *)((longlong)puVar10 + 0x8c) = uVar3;
  FUN_1808f78e0(local_418,local_828,param_1 + 0x340);
  return;
}



## raw_result.limitations



## normalized_result


void FUN_1808f03f0(longlong param_1,undefined8 *param_2,undefined8 *param_3)

{
  undefined4 uVar1;
  undefined4 uVar2;
  undefined4 uVar3;
  undefined4 uVar4;
  undefined8 uVar5;
  undefined8 uVar6;
  undefined8 uVar7;
  undefined8 uVar8;
  undefined8 *puVar9;
  undefined8 *puVar10;
  undefined8 *puVar11;
  longlong lVar12;
  longlong lVar13;
  undefined8 local_828 [130];
  undefined8 local_418 [131];
  
  lVar12 = 8;
  lVar13 = 8;
  puVar9 = local_828;
  do {
    puVar11 = param_3;
    puVar10 = puVar9;
    uVar5 = puVar11[1];
    uVar6 = puVar11[2];
    uVar7 = puVar11[3];
    *puVar10 = *puVar11;
    puVar10[1] = uVar5;
    uVar5 = puVar11[4];
    uVar8 = puVar11[5];
    puVar10[2] = uVar6;
    puVar10[3] = uVar7;
    uVar6 = puVar11[6];
    uVar7 = puVar11[7];
    puVar10[4] = uVar5;
    puVar10[5] = uVar8;
    uVar5 = puVar11[8];
    uVar8 = puVar11[9];
    puVar10[6] = uVar6;
    puVar10[7] = uVar7;
    uVar6 = puVar11[10];
    uVar7 = puVar11[0xb];
    puVar10[8] = uVar5;
    puVar10[9] = uVar8;
    uVar5 = puVar11[0xc];
    uVar8 = puVar11[0xd];
    puVar10[10] = uVar6;
    puVar10[0xb] = uVar7;
    uVar6 = puVar11[0xe];
    uVar7 = puVar11[0xf];
    puVar10[0xc] = uVar5;
    puVar10[0xd] = uVar8;
    puVar10[0xe] = uVar6;
    puVar10[0xf] = uVar7;
    lVar13 = lVar13 + -1;
    puVar9 = puVar10 + 0x10;
    param_3 = puVar11 + 0x10;
  } while (lVar13 != 0);
  uVar5 = puVar11[0x11];
  puVar10[0x10] = puVar11[0x10];
  puVar10[0x11] = uVar5;
  puVar9 = local_418;
  do {
    puVar11 = param_2;
    puVar10 = puVar9;
    uVar5 = puVar11[1];
    uVar6 = puVar11[2];
    uVar7 = puVar11[3];
    *puVar10 = *puVar11;
    puVar10[1] = uVar5;
    uVar5 = puVar11[4];
    uVar8 = puVar11[5];
    puVar10[2] = uVar6;
    puVar10[3] = uVar7;
    uVar6 = puVar11[6];
    uVar7 = puVar11[7];
    puVar10[4] = uVar5;
    puVar10[5] = uVar8;
    uVar5 = puVar11[8];
    uVar8 = puVar11[9];
    puVar10[6] = uVar6;
    puVar10[7] = uVar7;
    uVar6 = puVar11[10];
    uVar7 = puVar11[0xb];
    puVar10[8] = uVar5;
    puVar10[9] = uVar8;
    uVar5 = puVar11[0xc];
    uVar8 = puVar11[0xd];
    puVar10[10] = uVar6;
    puVar10[0xb] = uVar7;
    uVar1 = *(undefined4 *)(puVar11 + 0xe);
    uVar2 = *(undefined4 *)((longlong)puVar11 + 0x74);
    uVar3 = *(undefined4 *)(puVar11 + 0xf);
    uVar4 = *(undefined4 *)((longlong)puVar11 + 0x7c);
    puVar10[0xc] = uVar5;
    puVar10[0xd] = uVar8;
    *(undefined4 *)(puVar10 + 0xe) = uVar1;
    *(undefined4 *)((longlong)puVar10 + 0x74) = uVar2;
    *(undefined4 *)(puVar10 + 0xf) = uVar3;
    *(undefined4 *)((longlong)puVar10 + 0x7c) = uVar4;
    lVar12 = lVar12 + -1;
    puVar9 = puVar10 + 0x10;
    param_2 = puVar11 + 0x10;
  } while (lVar12 != 0);
  uVar1 = *(undefined4 *)((longlong)puVar11 + 0x84);
  uVar2 = *(undefined4 *)(puVar11 + 0x11);
  uVar3 = *(undefined4 *)((longlong)puVar11 + 0x8c);
  *(undefined4 *)(puVar10 + 0x10) = *(undefined4 *)(puVar11 + 0x10);
  *(undefined4 *)((longlong)puVar10 + 0x84) = uVar1;
  *(undefined4 *)(puVar10 + 0x11) = uVar2;
  *(undefined4 *)((longlong)puVar10 + 0x8c) = uVar3;
  FUN_1808f78e0(local_418,local_828,param_1 + 0x340);
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

