## evidence_id

ev_0e0b069962c4a2c71ac3feee1fde3dfdac02e487433d3ea186705edafa57a355

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
| procedure | 0x1808ab6b0 |

## raw_result.value


/* WARNING: Type propagation algorithm not settling */

void FUN_1808ab6b0(longlong *param_1,longlong *param_2,undefined8 *param_3,longlong param_4,
                  float param_5,longlong *param_6)

{
  char cVar1;
  undefined4 uVar2;
  float fVar3;
  int iVar4;
  undefined8 uVar5;
  uint *puVar6;
  char cVar7;
  uint uVar8;
  uint **ppuVar9;
  uint **ppuVar10;
  uint *puVar11;
  uint uVar12;
  longlong lVar13;
  longlong lVar14;
  longlong *plVar15;
  int iVar16;
  float *pfVar17;
  uint *puVar18;
  char *pcVar19;
  ulonglong uVar20;
  bool bVar21;
  float fVar22;
  float fVar23;
  undefined4 auStackX_20 [2];
  float afStack_558 [4];
  uint *local_548;
  undefined8 uStack_540;
  int local_538;
  char *local_530;
  uint *local_528;
  int iStack_520;
  int iStack_51c;
  float afStack_518 [2];
  char *local_510;
  uint *puStack_508;
  int iStack_500;
  int iStack_4fc;
  longlong local_4f8 [4];
  longlong local_4d8;
  longlong lStack_4d0;
  longlong local_4c8;
  longlong lStack_4c0;
  longlong local_4b8;
  longlong lStack_4b0;
  longlong local_4a8;
  longlong lStack_4a0;
  longlong local_498;
  longlong lStack_490;
  undefined1 local_488 [16];
  undefined1 local_478 [16];
  undefined4 uStack_f8;
  int iStack_f4;
  longlong lStack_f0;
  
  local_4f8[0] = *param_1;
  local_4f8[1] = param_1[1];
  local_4f8[2] = param_1[2];
  local_4f8[3] = param_1[3];
  local_4d8 = param_1[4];
  lStack_4d0 = param_1[5];
  local_4b8 = param_1[8];
  lStack_4b0 = param_1[9];
  local_4c8 = param_1[6];
  lStack_4c0 = param_1[7];
  local_498 = param_1[0xc];
  lStack_490 = param_1[0xd];
  local_4a8 = param_1[10];
  lStack_4a0 = param_1[0xb];
  local_478 = *(undefined1 (*) [16])(param_1 + 0x10);
  local_488 = *(undefined1 (*) [16])(param_1 + 0xe);
  FUN_1808a6d90(local_4f8);
  cVar1 = *(char *)(param_4 + 0x52);
  pcVar19 = (char *)(param_4 + 0xb8);
  iStack_520 = *(int *)((longlong)param_1 + 0x84);
  iStack_51c = iStack_520 * 4;
  local_538 = (int)param_6[3];
  local_528 = (uint *)(*param_6 + (longlong)local_538 * 4);
  uVar12 = iStack_520 + local_538;
  local_510 = (char *)(param_4 + 0x1b8);
  uVar8 = (uVar12 - (uVar12 & 0xf)) + 0x10;
  if ((uVar12 & 0xf) == 0) {
    uVar8 = uVar12;
  }
  *(uint *)(param_6 + 3) = uVar8;
  if (pcVar19 == local_510) {
    *(int *)(param_6 + 3) = local_538;
    return;
  }
  do {
    fVar23 = *(float *)(pcVar19 + 8);
    local_530 = pcVar19;
    if ((((*pcVar19 != '\0') && (pcVar19[1] != '\0')) && (*(int *)(pcVar19 + 0x1c) - 2U < 2)) &&
       (0.0 < fVar23)) {
      uVar2 = *(undefined4 *)(pcVar19 + 4);
      fVar3 = *(float *)(pcVar19 + 0xc);
      fVar22 = (float)FUN_181cbdf81(fVar3);
      iVar16 = iStack_520;
      puVar18 = local_528;
      local_4f8[0] = *param_2;
      local_4f8[1] = param_2[1];
      local_4f8[2] = param_2[2];
      local_4f8[3] = param_2[3];
      uVar5 = CONCAT44(iStack_51c,iStack_520);
      local_4c8 = param_2[6];
      lStack_4c0 = param_2[7];
      fVar22 = fVar22 - 1.0;
      local_4d8 = param_2[4];
      lStack_4d0 = param_2[5];
      local_4b8 = param_2[8];
      lStack_4b0 = param_2[9];
      if (fVar22 <= 0.0) {
        fVar22 = 0.0;
      }
      local_4a8 = param_2[10];
      lStack_4a0 = param_2[0xb];
      local_498 = param_2[0xc];
      lStack_490 = param_2[0xd];
      fVar23 = fVar23 * 2.0;
      if (6.0 <= fVar23) {
        fVar23 = 6.0;
      }
      local_488 = *(undefined1 (*) [16])(param_2 + 0xe);
      local_478 = *(undefined1 (*) [16])(param_2 + 0x10);
      local_548 = local_528;
      uStack_540 = uVar5;
      FUN_1808af3d0(&local_548,local_4f8,uVar2,fVar3 * 2.0,fVar22 * param_5 * fVar23);
      local_548 = puVar18;
      uStack_540 = uVar5;
      thunk_FUN_181dae2a0(puVar18,iVar16);
      iVar16 = iStack_520;
      if ((int)param_1[0x10] == 1) {
        iStack_500 = *(int *)((longlong)param_1 + 0x84);
        local_548 = (uint *)*param_1;
        iStack_4fc = iStack_500 * 4;
        uStack_540 = CONCAT44(iStack_4fc,iStack_500);
        puStack_508 = local_548;
        thunk_FUN_181dae2a0(puVar18,local_548,iStack_500);
        local_530 = pcVar19;
      }
      else {
        afStack_558[0] = 1.0;
        afStack_558[0] = *(float *)(pcVar19 + 0x14) * 2.0;
        fVar23 = (1.0 - *(float *)(pcVar19 + 0x14)) * 2.0;
        local_548 = (uint *)CONCAT44(local_548._4_4_,fVar23);
        ppuVar10 = (uint **)auStackX_20;
        if (0.0 <= fVar23) {
          ppuVar10 = &local_548;
        }
        ppuVar9 = (uint **)afStack_558;
        if (fVar23 <= 1.0) {
          ppuVar9 = ppuVar10;
        }
        ppuVar10 = (uint **)auStackX_20;
        if (0.0 <= afStack_558[0]) {
          ppuVar10 = (uint **)afStack_558;
        }
        afStack_518[0] = *(float *)ppuVar9;
        local_548 = (uint *)CONCAT44(local_548._4_4_,0x3f800000);
        auStackX_20[0] = 0;
        ppuVar9 = &local_548;
        if (afStack_558[0] <= 1.0) {
          ppuVar9 = ppuVar10;
        }
        afStack_518[1] = *(float *)ppuVar9;
        iVar4 = *(int *)(pcVar19 + 0x18);
        local_4f8[0] = 0;
        afStack_558[0] = (float)param_6[3];
        lVar14 = (longlong)iStack_520;
        FUN_1817bb8e2(local_4f8 + 1,0);
        puVar18 = local_528;
        uVar12 = *(uint *)(param_6 + 3);
        lStack_f0 = lVar14 << 2;
        lVar14 = 0;
        uStack_f8 = 2;
        iStack_f4 = iVar16;
        do {
          uVar8 = iVar16 + uVar12;
          local_4f8[lVar14] = *param_6 + (longlong)(int)uVar12 * 4;
          uVar12 = (uVar8 - (uVar8 & 0xf)) + 0x10;
          if ((uVar8 & 0xf) == 0) {
            uVar12 = uVar8;
          }
          lVar14 = lVar14 + 1;
          *(uint *)(param_6 + 3) = uVar12;
        } while (lVar14 < 2);
        plVar15 = local_4f8;
        pfVar17 = afStack_518;
        lVar14 = 2;
        do {
          if (0.01 <= *pfVar17) {
            if (*pfVar17 <= 0.99) {
              thunk_FUN_181dae2a0(puVar18);
            }
            else {
              thunk_FUN_181dae2a0(puVar18,*plVar15,iVar16);
            }
          }
          else {
            thunk_FUN_181dae2a0(0x3f800000,*plVar15,iVar16);
          }
          pfVar17 = pfVar17 + 1;
          plVar15 = plVar15 + 1;
          lVar14 = lVar14 + -1;
        } while (lVar14 != 0);
        puVar18 = (uint *)*param_3;
        local_548 = (uint *)param_3[1];
        if (puVar18 != (uint *)param_3[1]) {
          do {
            uVar12 = *puVar18;
            uVar20 = (ulonglong)uVar12;
            uVar8 = *(uint *)(local_530 + 0x18);
            if (uVar8 == 0) goto LAB_1808abc32;
            if (uVar8 == 1) {
              bVar21 = uVar12 != 10;
LAB_1808abc2e:
              if (bVar21) {
LAB_1808abc32:
                puVar6 = local_528;
                iVar16 = 0;
                lVar14 = 0;
                if (0 < (int)puVar18[4]) {
                  uVar2 = *(undefined4 *)((longlong)param_1 + 0x84);
                  do {
                    if ((puVar18[4] == 2) || ((cVar1 != '\0' && (1 < iVar4 - 9U)))) {
                      puVar11 = (uint *)local_4f8[lVar14];
                      lVar13 = param_1[*(longlong *)(puVar18 + 2) + lVar14];
                    }
                    else {
                      lVar13 = param_1[*(longlong *)(puVar18 + 2) + lVar14];
                      puVar11 = puVar6;
                    }
                    thunk_FUN_181dae2a0(puVar11,lVar13,uVar2);
                    iVar16 = iVar16 + 1;
                    lVar14 = lVar14 + 1;
                  } while (iVar16 < (int)puVar18[4]);
                }
              }
            }
            else {
              if (uVar8 == 2) {
                bVar21 = uVar12 != 9;
                goto LAB_1808abc2e;
              }
              if (uVar8 == 3) {
                if (uVar12 - 9 < 2) goto LAB_1808abc8e;
                goto LAB_1808abc32;
              }
              if (uVar8 == 5) {
                if (uVar12 - 0x10 < 3) goto LAB_1808abc32;
              }
              else if (uVar8 == 4) {
                if (2 < uVar12 - 8) goto LAB_1808abc32;
              }
              else if (uVar8 == 6) {
                cVar7 = FUN_18061d160(4,uVar20);
                if ((cVar7 != '\0') && (cVar7 = FUN_18061d160(5,uVar20 & 0xffffffff), cVar7 == '\0')
                   ) goto LAB_1808abc32;
              }
              else {
                if (uVar8 != 7) {
                  bVar21 = uVar8 == uVar12;
                  goto LAB_1808abc2e;
                }
                if (uVar12 - 8 < 2) goto LAB_1808abc32;
              }
            }
LAB_1808abc8e:
            puVar18 = puVar18 + 6;
          } while (puVar18 != local_548);
        }
        *(float *)(param_6 + 3) = afStack_558[0];
      }
    }
    pcVar19 = local_530 + 0x20;
    if (pcVar19 == local_510) {
      *(int *)(param_6 + 3) = local_538;
      return;
    }
  } while( true );
}



## raw_result.limitations



## normalized_result


/* WARNING: Type propagation algorithm not settling */

void FUN_1808ab6b0(longlong *param_1,longlong *param_2,undefined8 *param_3,longlong param_4,
                  float param_5,longlong *param_6)

{
  char cVar1;
  undefined4 uVar2;
  float fVar3;
  int iVar4;
  undefined8 uVar5;
  uint *puVar6;
  char cVar7;
  uint uVar8;
  uint **ppuVar9;
  uint **ppuVar10;
  uint *puVar11;
  uint uVar12;
  longlong lVar13;
  longlong lVar14;
  longlong *plVar15;
  int iVar16;
  float *pfVar17;
  uint *puVar18;
  char *pcVar19;
  ulonglong uVar20;
  bool bVar21;
  float fVar22;
  float fVar23;
  undefined4 auStackX_20 [2];
  float afStack_558 [4];
  uint *local_548;
  undefined8 uStack_540;
  int local_538;
  char *local_530;
  uint *local_528;
  int iStack_520;
  int iStack_51c;
  float afStack_518 [2];
  char *local_510;
  uint *puStack_508;
  int iStack_500;
  int iStack_4fc;
  longlong local_4f8 [4];
  longlong local_4d8;
  longlong lStack_4d0;
  longlong local_4c8;
  longlong lStack_4c0;
  longlong local_4b8;
  longlong lStack_4b0;
  longlong local_4a8;
  longlong lStack_4a0;
  longlong local_498;
  longlong lStack_490;
  undefined1 local_488 [16];
  undefined1 local_478 [16];
  undefined4 uStack_f8;
  int iStack_f4;
  longlong lStack_f0;
  
  local_4f8[0] = *param_1;
  local_4f8[1] = param_1[1];
  local_4f8[2] = param_1[2];
  local_4f8[3] = param_1[3];
  local_4d8 = param_1[4];
  lStack_4d0 = param_1[5];
  local_4b8 = param_1[8];
  lStack_4b0 = param_1[9];
  local_4c8 = param_1[6];
  lStack_4c0 = param_1[7];
  local_498 = param_1[0xc];
  lStack_490 = param_1[0xd];
  local_4a8 = param_1[10];
  lStack_4a0 = param_1[0xb];
  local_478 = *(undefined1 (*) [16])(param_1 + 0x10);
  local_488 = *(undefined1 (*) [16])(param_1 + 0xe);
  FUN_1808a6d90(local_4f8);
  cVar1 = *(char *)(param_4 + 0x52);
  pcVar19 = (char *)(param_4 + 0xb8);
  iStack_520 = *(int *)((longlong)param_1 + 0x84);
  iStack_51c = iStack_520 * 4;
  local_538 = (int)param_6[3];
  local_528 = (uint *)(*param_6 + (longlong)local_538 * 4);
  uVar12 = iStack_520 + local_538;
  local_510 = (char *)(param_4 + 0x1b8);
  uVar8 = (uVar12 - (uVar12 & 0xf)) + 0x10;
  if ((uVar12 & 0xf) == 0) {
    uVar8 = uVar12;
  }
  *(uint *)(param_6 + 3) = uVar8;
  if (pcVar19 == local_510) {
    *(int *)(param_6 + 3) = local_538;
    return;
  }
  do {
    fVar23 = *(float *)(pcVar19 + 8);
    local_530 = pcVar19;
    if ((((*pcVar19 != '\0') && (pcVar19[1] != '\0')) && (*(int *)(pcVar19 + 0x1c) - 2U < 2)) &&
       (0.0 < fVar23)) {
      uVar2 = *(undefined4 *)(pcVar19 + 4);
      fVar3 = *(float *)(pcVar19 + 0xc);
      fVar22 = (float)FUN_181cbdf81(fVar3);
      iVar16 = iStack_520;
      puVar18 = local_528;
      local_4f8[0] = *param_2;
      local_4f8[1] = param_2[1];
      local_4f8[2] = param_2[2];
      local_4f8[3] = param_2[3];
      uVar5 = CONCAT44(iStack_51c,iStack_520);
      local_4c8 = param_2[6];
      lStack_4c0 = param_2[7];
      fVar22 = fVar22 - 1.0;
      local_4d8 = param_2[4];
      lStack_4d0 = param_2[5];
      local_4b8 = param_2[8];
      lStack_4b0 = param_2[9];
      if (fVar22 <= 0.0) {
        fVar22 = 0.0;
      }
      local_4a8 = param_2[10];
      lStack_4a0 = param_2[0xb];
      local_498 = param_2[0xc];
      lStack_490 = param_2[0xd];
      fVar23 = fVar23 * 2.0;
      if (6.0 <= fVar23) {
        fVar23 = 6.0;
      }
      local_488 = *(undefined1 (*) [16])(param_2 + 0xe);
      local_478 = *(undefined1 (*) [16])(param_2 + 0x10);
      local_548 = local_528;
      uStack_540 = uVar5;
      FUN_1808af3d0(&local_548,local_4f8,uVar2,fVar3 * 2.0,fVar22 * param_5 * fVar23);
      local_548 = puVar18;
      uStack_540 = uVar5;
      thunk_FUN_181dae2a0(puVar18,iVar16);
      iVar16 = iStack_520;
      if ((int)param_1[0x10] == 1) {
        iStack_500 = *(int *)((longlong)param_1 + 0x84);
        local_548 = (uint *)*param_1;
        iStack_4fc = iStack_500 * 4;
        uStack_540 = CONCAT44(iStack_4fc,iStack_500);
        puStack_508 = local_548;
        thunk_FUN_181dae2a0(puVar18,local_548,iStack_500);
        local_530 = pcVar19;
      }
      else {
        afStack_558[0] = 1.0;
        afStack_558[0] = *(float *)(pcVar19 + 0x14) * 2.0;
        fVar23 = (1.0 - *(float *)(pcVar19 + 0x14)) * 2.0;
        local_548 = (uint *)CONCAT44(local_548._4_4_,fVar23);
        ppuVar10 = (uint **)auStackX_20;
        if (0.0 <= fVar23) {
          ppuVar10 = &local_548;
        }
        ppuVar9 = (uint **)afStack_558;
        if (fVar23 <= 1.0) {
          ppuVar9 = ppuVar10;
        }
        ppuVar10 = (uint **)auStackX_20;
        if (0.0 <= afStack_558[0]) {
          ppuVar10 = (uint **)afStack_558;
        }
        afStack_518[0] = *(float *)ppuVar9;
        local_548 = (uint *)CONCAT44(local_548._4_4_,0x3f800000);
        auStackX_20[0] = 0;
        ppuVar9 = &local_548;
        if (afStack_558[0] <= 1.0) {
          ppuVar9 = ppuVar10;
        }
        afStack_518[1] = *(float *)ppuVar9;
        iVar4 = *(int *)(pcVar19 + 0x18);
        local_4f8[0] = 0;
        afStack_558[0] = (float)param_6[3];
        lVar14 = (longlong)iStack_520;
        FUN_1817bb8e2(local_4f8 + 1,0);
        puVar18 = local_528;
        uVar12 = *(uint *)(param_6 + 3);
        lStack_f0 = lVar14 << 2;
        lVar14 = 0;
        uStack_f8 = 2;
        iStack_f4 = iVar16;
        do {
          uVar8 = iVar16 + uVar12;
          local_4f8[lVar14] = *param_6 + (longlong)(int)uVar12 * 4;
          uVar12 = (uVar8 - (uVar8 & 0xf)) + 0x10;
          if ((uVar8 & 0xf) == 0) {
            uVar12 = uVar8;
          }
          lVar14 = lVar14 + 1;
          *(uint *)(param_6 + 3) = uVar12;
        } while (lVar14 < 2);
        plVar15 = local_4f8;
        pfVar17 = afStack_518;
        lVar14 = 2;
        do {
          if (0.01 <= *pfVar17) {
            if (*pfVar17 <= 0.99) {
              thunk_FUN_181dae2a0(puVar18);
            }
            else {
              thunk_FUN_181dae2a0(puVar18,*plVar15,iVar16);
            }
          }
          else {
            thunk_FUN_181dae2a0(0x3f800000,*plVar15,iVar16);
          }
          pfVar17 = pfVar17 + 1;
          plVar15 = plVar15 + 1;
          lVar14 = lVar14 + -1;
        } while (lVar14 != 0);
        puVar18 = (uint *)*param_3;
        local_548 = (uint *)param_3[1];
        if (puVar18 != (uint *)param_3[1]) {
          do {
            uVar12 = *puVar18;
            uVar20 = (ulonglong)uVar12;
            uVar8 = *(uint *)(local_530 + 0x18);
            if (uVar8 == 0) goto LAB_1808abc32;
            if (uVar8 == 1) {
              bVar21 = uVar12 != 10;
LAB_1808abc2e:
              if (bVar21) {
LAB_1808abc32:
                puVar6 = local_528;
                iVar16 = 0;
                lVar14 = 0;
                if (0 < (int)puVar18[4]) {
                  uVar2 = *(undefined4 *)((longlong)param_1 + 0x84);
                  do {
                    if ((puVar18[4] == 2) || ((cVar1 != '\0' && (1 < iVar4 - 9U)))) {
                      puVar11 = (uint *)local_4f8[lVar14];
                      lVar13 = param_1[*(longlong *)(puVar18 + 2) + lVar14];
                    }
                    else {
                      lVar13 = param_1[*(longlong *)(puVar18 + 2) + lVar14];
                      puVar11 = puVar6;
                    }
                    thunk_FUN_181dae2a0(puVar11,lVar13,uVar2);
                    iVar16 = iVar16 + 1;
                    lVar14 = lVar14 + 1;
                  } while (iVar16 < (int)puVar18[4]);
                }
              }
            }
            else {
              if (uVar8 == 2) {
                bVar21 = uVar12 != 9;
                goto LAB_1808abc2e;
              }
              if (uVar8 == 3) {
                if (uVar12 - 9 < 2) goto LAB_1808abc8e;
                goto LAB_1808abc32;
              }
              if (uVar8 == 5) {
                if (uVar12 - 0x10 < 3) goto LAB_1808abc32;
              }
              else if (uVar8 == 4) {
                if (2 < uVar12 - 8) goto LAB_1808abc32;
              }
              else if (uVar8 == 6) {
                cVar7 = FUN_18061d160(4,uVar20);
                if ((cVar7 != '\0') && (cVar7 = FUN_18061d160(5,uVar20 & 0xffffffff), cVar7 == '\0')
                   ) goto LAB_1808abc32;
              }
              else {
                if (uVar8 != 7) {
                  bVar21 = uVar8 == uVar12;
                  goto LAB_1808abc2e;
                }
                if (uVar12 - 8 < 2) goto LAB_1808abc32;
              }
            }
LAB_1808abc8e:
            puVar18 = puVar18 + 6;
          } while (puVar18 != local_548);
        }
        *(float *)(param_6 + 3) = afStack_558[0];
      }
    }
    pcVar19 = local_530 + 0x20;
    if (pcVar19 == local_510) {
      *(int *)(param_6 + 3) = local_538;
      return;
    }
  } while( true );
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

