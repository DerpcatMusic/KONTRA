## evidence_id

ev_41c047a1301283fb30182c7d524e1bffd1d37ee4258dc3bab289c3cc54b7d8f4

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
| procedure | 0x1808aff30 |

## raw_result.value


void FUN_1808aff30(undefined8 *param_1,undefined8 *param_2,undefined8 *param_3,longlong param_4,
                  undefined8 *param_5,float param_6,char param_7,longlong *param_8)

{
  undefined1 (*pauVar1) [16];
  uint *puVar2;
  longlong lVar3;
  int iVar4;
  longlong *plVar5;
  undefined1 auVar6 [16];
  char cVar7;
  uint uVar8;
  float *pfVar9;
  uint uVar10;
  float *pfVar11;
  longlong *plVar12;
  undefined8 uVar13;
  longlong lVar14;
  undefined8 *puVar15;
  int iVar16;
  ulonglong uVar17;
  undefined4 uVar18;
  ulonglong uVar19;
  uint *puVar20;
  longlong lVar21;
  bool bVar22;
  float fVar23;
  float local_148 [2];
  longlong local_140;
  uint *local_138;
  undefined8 uStack_130;
  undefined8 local_128 [4];
  undefined8 local_108;
  undefined8 uStack_100;
  undefined8 local_f8;
  undefined8 uStack_f0;
  undefined8 local_e8;
  undefined8 uStack_e0;
  undefined8 local_d8;
  undefined8 uStack_d0;
  undefined8 local_c8;
  undefined8 uStack_c0;
  undefined8 local_b8;
  undefined8 uStack_b0;
  undefined1 local_a8 [16];
  undefined4 local_98;
  float afStack_90 [2];
  undefined8 local_88;
  
  plVar5 = param_8;
  iVar4 = *(int *)((longlong)param_1 + 0x84);
  uVar10 = (int)param_8[3] + iVar4;
  local_98 = (int)param_8[3];
  uStack_130 = CONCAT44(iVar4 * 4,iVar4);
  puVar2 = (uint *)(*param_8 + (longlong)(int)param_8[3] * 4);
  local_138 = puVar2;
  uVar8 = (uVar10 - (uVar10 & 0xf)) + 0x10;
  if ((uVar10 & 0xf) == 0) {
    uVar8 = uVar10;
  }
  *(uint *)(param_8 + 3) = uVar8;
  iVar16 = *(int *)(param_4 + 0x1c);
  switch(iVar16) {
  case 0:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808ba350(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10))
    ;
    break;
  case 1:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808b9be0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10),
                  *(undefined4 *)(param_4 + 8));
    break;
  case 2:
    local_140 = CONCAT44(local_140._4_4_,0x41200000);
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_148[0] = ABS(*(float *)(param_4 + 8));
    param_8._0_4_ = 0.0;
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    pfVar11 = (float *)&param_8;
    if (0.0 <= local_148[0]) {
      pfVar11 = local_148;
    }
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    pfVar9 = (float *)&local_140;
    if (local_148[0] <= 10.0) {
      pfVar9 = pfVar11;
    }
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_138 = puVar2;
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808af3d0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0xc),
                  SQRT(*pfVar9 * 0.1) * param_6 * *(float *)(param_4 + 8));
    thunk_FUN_181dae2a0(puVar2,iVar4);
    local_128[2] = param_3[2];
    local_128[3] = param_3[3];
    local_108 = param_3[4];
    uStack_100 = param_3[5];
    local_f8 = param_3[6];
    uStack_f0 = param_3[7];
    local_d8 = param_3[10];
    uStack_d0 = param_3[0xb];
    local_e8 = param_3[8];
    uStack_e0 = param_3[9];
    local_b8 = param_3[0xe];
    uStack_b0 = param_3[0xf];
    local_c8 = param_3[0xc];
    uStack_c0 = param_3[0xd];
    if (*(int *)(param_3 + 0x10) == 1) {
      thunk_FUN_181dae2a0(puVar2,*param_3,puVar2,iVar4);
    }
    break;
  case 3:
    local_148[0] = 10.0;
    fVar23 = ABS(*(float *)(param_4 + 8));
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    pfVar11 = (float *)&param_8;
    if (0.0 <= fVar23) {
      pfVar11 = (float *)&local_140;
    }
    pfVar9 = local_148;
    if (fVar23 <= 10.0) {
      pfVar9 = pfVar11;
    }
    fVar23 = *(float *)(param_4 + 0xc);
    param_8._0_4_ = 0.25;
    plVar12 = (longlong *)&param_8;
    if (fVar23 <= 0.25) {
      plVar12 = &local_140;
    }
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    FUN_1808b9da0(&local_138,local_128,*(undefined4 *)(param_4 + 4),fVar23,(int)*plVar12,
                  SQRT(*pfVar9 * 0.1) * param_6 * *(float *)(param_4 + 8),plVar5);
    break;
  case 4:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    fVar23 = *(float *)(param_4 + 0xc) * 0.707;
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    if (0.25 <= fVar23) {
      fVar23 = 0.25;
    }
    FUN_1808b9da0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(float *)(param_4 + 0xc),fVar23
                  ,0,param_8);
    thunk_FUN_181dae2a0();
    thunk_FUN_181dae2a0();
    thunk_FUN_181dae2a0(puVar2,iVar4);
    break;
  case 5:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    FUN_1808ba720(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10),
                  *(float *)(param_4 + 8) * 2.0);
    break;
  case 6:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808b9cc0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10),
                  *(undefined4 *)(param_4 + 8));
    break;
  case 7:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808ba540(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10))
    ;
  }
  thunk_FUN_181dae2a0(puVar2,iVar4);
  if (*(int *)(param_1 + 0x10) == 1) {
    local_128[0] = *param_1;
    local_128[2] = param_1[2];
    local_128[3] = param_1[3];
    local_108 = param_1[4];
    uStack_100 = param_1[5];
    local_f8 = param_1[6];
    uStack_f0 = param_1[7];
    local_e8 = param_1[8];
    uStack_e0 = param_1[9];
    local_d8 = param_1[10];
    uStack_d0 = param_1[0xb];
    local_c8 = param_1[0xc];
    uStack_c0 = param_1[0xd];
    local_a8 = *(undefined1 (*) [16])(param_1 + 0x10);
    local_b8 = param_1[0xe];
    uStack_b0 = param_1[0xf];
    uVar8 = local_a8._0_4_;
    local_128[1] = param_1[1];
    uVar18 = local_a8._4_4_;
    if (iVar16 == 3) {
      if (uVar8 == 1) {
        thunk_FUN_181dae2a0(puVar2,local_128[0],uVar18);
      }
      else if (0 < (int)uVar8) {
        puVar15 = local_128;
        uVar17 = (ulonglong)uVar8;
        do {
          thunk_FUN_181dae2a0(puVar2,*puVar15,uVar18);
          puVar15 = puVar15 + 1;
          uVar17 = uVar17 - 1;
        } while (uVar17 != 0);
      }
    }
    else if (uVar8 == 1) {
      thunk_FUN_181dae2a0(puVar2,local_128[0],uVar18);
    }
    else if (0 < (int)uVar8) {
      puVar15 = local_128;
      uVar17 = (ulonglong)uVar8;
      do {
        thunk_FUN_181dae2a0(puVar2,*puVar15,uVar18);
        puVar15 = puVar15 + 1;
        uVar17 = uVar17 - 1;
      } while (uVar17 != 0);
    }
  }
  else {
    local_148[0] = 1.0;
    fVar23 = (1.0 - *(float *)(param_4 + 0x14)) * 2.0;
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    pfVar11 = (float *)&param_8;
    if (0.0 <= fVar23) {
      pfVar11 = (float *)&local_140;
    }
    pfVar9 = local_148;
    if (fVar23 <= 1.0) {
      pfVar9 = pfVar11;
    }
    afStack_90[0] = *pfVar9;
    local_148[0] = 1.0;
    fVar23 = *(float *)(param_4 + 0x14) * 2.0;
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    pfVar11 = (float *)&param_8;
    if (0.0 <= fVar23) {
      pfVar11 = (float *)&local_140;
    }
    pfVar9 = local_148;
    if (fVar23 <= 1.0) {
      pfVar9 = pfVar11;
    }
    afStack_90[1] = *pfVar9;
    local_140 = (longlong)iVar16;
    cVar7 = *(int *)(param_4 + 0x18) - 9U < 2;
    param_8._0_4_ = (float)(uint)(byte)cVar7;
    puVar20 = (uint *)*param_5;
    local_138 = (uint *)param_5[1];
    if (puVar20 != (uint *)param_5[1]) {
      do {
        uVar8 = *puVar20;
        uVar17 = (ulonglong)uVar8;
        uVar10 = *(uint *)(param_4 + 0x18);
        if (uVar10 == 0) goto LAB_1808b0920;
        if (uVar10 == 1) {
          bVar22 = uVar8 != 10;
LAB_1808b0995:
          if (bVar22) {
LAB_1808b0920:
            lVar21 = 0;
            local_148[0] = 0.0;
            if (0 < (int)puVar20[4]) {
              iVar4 = *(int *)((longlong)param_1 + 0x84);
              lVar14 = local_140;
              do {
                if ((puVar20[4] == 2) || ((param_7 != '\0' && (cVar7 == '\0')))) {
                  uVar13 = param_1[*(longlong *)(puVar20 + 2) + lVar21];
                  local_88 = uVar13;
                  if (lVar14 == 3) {
                    iVar16 = (int)plVar5[3];
                    lVar14 = *plVar5 + (longlong)iVar16 * 4;
                    uVar8 = iVar4 + iVar16;
                    uVar10 = (uVar8 - (uVar8 & 0xf)) + 0x10;
                    if ((uVar8 & 0xf) == 0) {
                      uVar10 = uVar8;
                    }
                    *(uint *)(plVar5 + 3) = uVar10;
                    thunk_FUN_181dae2a0(puVar2,3,lVar14,iVar4);
                    thunk_FUN_181dae2a0(lVar14,local_88,iVar4);
                  }
                  else {
                    fVar23 = afStack_90[lVar21];
                    if (fVar23 < 0.01) goto LAB_1808b0ae3;
                    if (0.99 < fVar23) goto LAB_1808b0a38;
                    iVar16 = (int)plVar5[3];
                    lVar3 = *plVar5 + (longlong)iVar16 * 4;
                    uVar8 = iVar4 + iVar16;
                    if (lVar14 == 4) {
                      uVar10 = (uVar8 - (uVar8 & 0xf)) + 0x10;
                      if (((longlong)(int)uVar8 & 0xfU) == 0) {
                        uVar10 = uVar8;
                      }
                      *(uint *)(plVar5 + 3) = uVar10;
                      thunk_FUN_181dae2a0(puVar2,(longlong)(int)uVar8,lVar3);
                      thunk_FUN_181dae2a0(1.0 - fVar23,lVar3,iVar4);
                    }
                    else {
                      uVar19 = (ulonglong)(int)uVar8;
                      uVar17 = (uVar19 - (uVar8 & 0xf)) + 0x10;
                      if ((uVar19 & 0xf) == 0) {
                        uVar17 = uVar19;
                      }
                      *(int *)(plVar5 + 3) = (int)uVar17;
                      thunk_FUN_181dae2a0(puVar2,uVar17,lVar3,iVar4);
                    }
                    thunk_FUN_181dae2a0(lVar3,local_88,iVar4);
                  }
                  *(int *)(plVar5 + 3) = iVar16;
                  lVar14 = local_140;
                }
                else {
                  uVar13 = param_1[*(longlong *)(puVar20 + 2) + lVar21];
                  if (lVar14 == 3) {
                    thunk_FUN_181dae2a0();
                    lVar14 = local_140;
                  }
                  else {
LAB_1808b0a38:
                    thunk_FUN_181dae2a0(puVar2,uVar13,iVar4);
                    lVar14 = local_140;
                  }
                }
LAB_1808b0ae3:
                lVar21 = lVar21 + 1;
                local_148[0] = (float)((int)local_148[0] + 1);
                cVar7 = (char)param_8;
              } while ((int)local_148[0] < (int)puVar20[4]);
            }
          }
        }
        else {
          if (uVar10 == 2) {
            bVar22 = uVar8 != 9;
            goto LAB_1808b0995;
          }
          if (uVar10 == 3) {
            if (uVar8 - 9 < 2) goto LAB_1808b0b02;
            goto LAB_1808b0920;
          }
          if (uVar10 == 5) {
            if (uVar8 - 0x10 < 3) goto LAB_1808b0920;
          }
          else if (uVar10 == 4) {
            if (2 < uVar8 - 8) goto LAB_1808b0920;
          }
          else if (uVar10 == 6) {
            cVar7 = FUN_18061d160(4,uVar17);
            if ((cVar7 != '\0') && (cVar7 = FUN_18061d160(5,uVar17 & 0xffffffff), cVar7 == '\0')) {
              cVar7 = (char)param_8;
              goto LAB_1808b0920;
            }
          }
          else {
            if (uVar10 != 7) {
              bVar22 = uVar10 == uVar8;
              goto LAB_1808b0995;
            }
            if (uVar8 - 8 < 2) goto LAB_1808b0920;
          }
        }
LAB_1808b0b02:
        puVar20 = puVar20 + 6;
        cVar7 = (char)param_8;
      } while (puVar20 != local_138);
    }
  }
  FUN_181cbdb35(0x4024000000000000,0x4008000000000000);
  local_128[0] = *param_1;
  local_128[1] = param_1[1];
  local_108 = param_1[4];
  uStack_100 = param_1[5];
  local_128[2] = param_1[2];
  local_128[3] = param_1[3];
  local_e8 = param_1[8];
  uStack_e0 = param_1[9];
  local_f8 = param_1[6];
  uStack_f0 = param_1[7];
  local_c8 = param_1[0xc];
  uStack_c0 = param_1[0xd];
  pauVar1 = (undefined1 (*) [16])(param_1 + 0x10);
  local_d8 = param_1[10];
  uStack_d0 = param_1[0xb];
  local_b8 = param_1[0xe];
  uStack_b0 = param_1[0xf];
  local_a8 = *pauVar1;
  auVar6 = local_a8;
  if (0 < (int)*(uint *)*pauVar1) {
    local_a8._4_4_ = SUB124(*(undefined1 (*) [12])*pauVar1,4);
    uVar18 = local_a8._4_4_;
    puVar15 = local_128;
    uVar17 = (ulonglong)*(uint *)*pauVar1;
    local_a8 = auVar6;
    do {
      thunk_FUN_181dae2a0(*puVar15,uVar18);
      puVar15 = puVar15 + 1;
      uVar17 = uVar17 - 1;
    } while (uVar17 != 0);
  }
  *(undefined4 *)(plVar5 + 3) = local_98;
  return;
}



## raw_result.limitations



## normalized_result


void FUN_1808aff30(undefined8 *param_1,undefined8 *param_2,undefined8 *param_3,longlong param_4,
                  undefined8 *param_5,float param_6,char param_7,longlong *param_8)

{
  undefined1 (*pauVar1) [16];
  uint *puVar2;
  longlong lVar3;
  int iVar4;
  longlong *plVar5;
  undefined1 auVar6 [16];
  char cVar7;
  uint uVar8;
  float *pfVar9;
  uint uVar10;
  float *pfVar11;
  longlong *plVar12;
  undefined8 uVar13;
  longlong lVar14;
  undefined8 *puVar15;
  int iVar16;
  ulonglong uVar17;
  undefined4 uVar18;
  ulonglong uVar19;
  uint *puVar20;
  longlong lVar21;
  bool bVar22;
  float fVar23;
  float local_148 [2];
  longlong local_140;
  uint *local_138;
  undefined8 uStack_130;
  undefined8 local_128 [4];
  undefined8 local_108;
  undefined8 uStack_100;
  undefined8 local_f8;
  undefined8 uStack_f0;
  undefined8 local_e8;
  undefined8 uStack_e0;
  undefined8 local_d8;
  undefined8 uStack_d0;
  undefined8 local_c8;
  undefined8 uStack_c0;
  undefined8 local_b8;
  undefined8 uStack_b0;
  undefined1 local_a8 [16];
  undefined4 local_98;
  float afStack_90 [2];
  undefined8 local_88;
  
  plVar5 = param_8;
  iVar4 = *(int *)((longlong)param_1 + 0x84);
  uVar10 = (int)param_8[3] + iVar4;
  local_98 = (int)param_8[3];
  uStack_130 = CONCAT44(iVar4 * 4,iVar4);
  puVar2 = (uint *)(*param_8 + (longlong)(int)param_8[3] * 4);
  local_138 = puVar2;
  uVar8 = (uVar10 - (uVar10 & 0xf)) + 0x10;
  if ((uVar10 & 0xf) == 0) {
    uVar8 = uVar10;
  }
  *(uint *)(param_8 + 3) = uVar8;
  iVar16 = *(int *)(param_4 + 0x1c);
  switch(iVar16) {
  case 0:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808ba350(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10))
    ;
    break;
  case 1:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808b9be0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10),
                  *(undefined4 *)(param_4 + 8));
    break;
  case 2:
    local_140 = CONCAT44(local_140._4_4_,0x41200000);
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_148[0] = ABS(*(float *)(param_4 + 8));
    param_8._0_4_ = 0.0;
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    pfVar11 = (float *)&param_8;
    if (0.0 <= local_148[0]) {
      pfVar11 = local_148;
    }
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    pfVar9 = (float *)&local_140;
    if (local_148[0] <= 10.0) {
      pfVar9 = pfVar11;
    }
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_138 = puVar2;
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808af3d0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0xc),
                  SQRT(*pfVar9 * 0.1) * param_6 * *(float *)(param_4 + 8));
    thunk_FUN_181dae2a0(puVar2,iVar4);
    local_128[2] = param_3[2];
    local_128[3] = param_3[3];
    local_108 = param_3[4];
    uStack_100 = param_3[5];
    local_f8 = param_3[6];
    uStack_f0 = param_3[7];
    local_d8 = param_3[10];
    uStack_d0 = param_3[0xb];
    local_e8 = param_3[8];
    uStack_e0 = param_3[9];
    local_b8 = param_3[0xe];
    uStack_b0 = param_3[0xf];
    local_c8 = param_3[0xc];
    uStack_c0 = param_3[0xd];
    if (*(int *)(param_3 + 0x10) == 1) {
      thunk_FUN_181dae2a0(puVar2,*param_3,puVar2,iVar4);
    }
    break;
  case 3:
    local_148[0] = 10.0;
    fVar23 = ABS(*(float *)(param_4 + 8));
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    pfVar11 = (float *)&param_8;
    if (0.0 <= fVar23) {
      pfVar11 = (float *)&local_140;
    }
    pfVar9 = local_148;
    if (fVar23 <= 10.0) {
      pfVar9 = pfVar11;
    }
    fVar23 = *(float *)(param_4 + 0xc);
    param_8._0_4_ = 0.25;
    plVar12 = (longlong *)&param_8;
    if (fVar23 <= 0.25) {
      plVar12 = &local_140;
    }
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    FUN_1808b9da0(&local_138,local_128,*(undefined4 *)(param_4 + 4),fVar23,(int)*plVar12,
                  SQRT(*pfVar9 * 0.1) * param_6 * *(float *)(param_4 + 8),plVar5);
    break;
  case 4:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    fVar23 = *(float *)(param_4 + 0xc) * 0.707;
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    if (0.25 <= fVar23) {
      fVar23 = 0.25;
    }
    FUN_1808b9da0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(float *)(param_4 + 0xc),fVar23
                  ,0,param_8);
    thunk_FUN_181dae2a0();
    thunk_FUN_181dae2a0();
    thunk_FUN_181dae2a0(puVar2,iVar4);
    break;
  case 5:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    FUN_1808ba720(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10),
                  *(float *)(param_4 + 8) * 2.0);
    break;
  case 6:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808b9cc0(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10),
                  *(undefined4 *)(param_4 + 8));
    break;
  case 7:
    local_128[0] = *param_2;
    local_128[1] = param_2[1];
    local_128[2] = param_2[2];
    local_128[3] = param_2[3];
    local_108 = param_2[4];
    uStack_100 = param_2[5];
    local_f8 = param_2[6];
    uStack_f0 = param_2[7];
    local_e8 = param_2[8];
    uStack_e0 = param_2[9];
    local_d8 = param_2[10];
    uStack_d0 = param_2[0xb];
    local_c8 = param_2[0xc];
    uStack_c0 = param_2[0xd];
    local_b8 = param_2[0xe];
    uStack_b0 = param_2[0xf];
    local_a8 = *(undefined1 (*) [16])(param_2 + 0x10);
    FUN_1808ba540(&local_138,local_128,*(undefined4 *)(param_4 + 4),*(undefined4 *)(param_4 + 0x10))
    ;
  }
  thunk_FUN_181dae2a0(puVar2,iVar4);
  if (*(int *)(param_1 + 0x10) == 1) {
    local_128[0] = *param_1;
    local_128[2] = param_1[2];
    local_128[3] = param_1[3];
    local_108 = param_1[4];
    uStack_100 = param_1[5];
    local_f8 = param_1[6];
    uStack_f0 = param_1[7];
    local_e8 = param_1[8];
    uStack_e0 = param_1[9];
    local_d8 = param_1[10];
    uStack_d0 = param_1[0xb];
    local_c8 = param_1[0xc];
    uStack_c0 = param_1[0xd];
    local_a8 = *(undefined1 (*) [16])(param_1 + 0x10);
    local_b8 = param_1[0xe];
    uStack_b0 = param_1[0xf];
    uVar8 = local_a8._0_4_;
    local_128[1] = param_1[1];
    uVar18 = local_a8._4_4_;
    if (iVar16 == 3) {
      if (uVar8 == 1) {
        thunk_FUN_181dae2a0(puVar2,local_128[0],uVar18);
      }
      else if (0 < (int)uVar8) {
        puVar15 = local_128;
        uVar17 = (ulonglong)uVar8;
        do {
          thunk_FUN_181dae2a0(puVar2,*puVar15,uVar18);
          puVar15 = puVar15 + 1;
          uVar17 = uVar17 - 1;
        } while (uVar17 != 0);
      }
    }
    else if (uVar8 == 1) {
      thunk_FUN_181dae2a0(puVar2,local_128[0],uVar18);
    }
    else if (0 < (int)uVar8) {
      puVar15 = local_128;
      uVar17 = (ulonglong)uVar8;
      do {
        thunk_FUN_181dae2a0(puVar2,*puVar15,uVar18);
        puVar15 = puVar15 + 1;
        uVar17 = uVar17 - 1;
      } while (uVar17 != 0);
    }
  }
  else {
    local_148[0] = 1.0;
    fVar23 = (1.0 - *(float *)(param_4 + 0x14)) * 2.0;
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    pfVar11 = (float *)&param_8;
    if (0.0 <= fVar23) {
      pfVar11 = (float *)&local_140;
    }
    pfVar9 = local_148;
    if (fVar23 <= 1.0) {
      pfVar9 = pfVar11;
    }
    afStack_90[0] = *pfVar9;
    local_148[0] = 1.0;
    fVar23 = *(float *)(param_4 + 0x14) * 2.0;
    local_140 = CONCAT44(local_140._4_4_,fVar23);
    pfVar11 = (float *)&param_8;
    if (0.0 <= fVar23) {
      pfVar11 = (float *)&local_140;
    }
    pfVar9 = local_148;
    if (fVar23 <= 1.0) {
      pfVar9 = pfVar11;
    }
    afStack_90[1] = *pfVar9;
    local_140 = (longlong)iVar16;
    cVar7 = *(int *)(param_4 + 0x18) - 9U < 2;
    param_8._0_4_ = (float)(uint)(byte)cVar7;
    puVar20 = (uint *)*param_5;
    local_138 = (uint *)param_5[1];
    if (puVar20 != (uint *)param_5[1]) {
      do {
        uVar8 = *puVar20;
        uVar17 = (ulonglong)uVar8;
        uVar10 = *(uint *)(param_4 + 0x18);
        if (uVar10 == 0) goto LAB_1808b0920;
        if (uVar10 == 1) {
          bVar22 = uVar8 != 10;
LAB_1808b0995:
          if (bVar22) {
LAB_1808b0920:
            lVar21 = 0;
            local_148[0] = 0.0;
            if (0 < (int)puVar20[4]) {
              iVar4 = *(int *)((longlong)param_1 + 0x84);
              lVar14 = local_140;
              do {
                if ((puVar20[4] == 2) || ((param_7 != '\0' && (cVar7 == '\0')))) {
                  uVar13 = param_1[*(longlong *)(puVar20 + 2) + lVar21];
                  local_88 = uVar13;
                  if (lVar14 == 3) {
                    iVar16 = (int)plVar5[3];
                    lVar14 = *plVar5 + (longlong)iVar16 * 4;
                    uVar8 = iVar4 + iVar16;
                    uVar10 = (uVar8 - (uVar8 & 0xf)) + 0x10;
                    if ((uVar8 & 0xf) == 0) {
                      uVar10 = uVar8;
                    }
                    *(uint *)(plVar5 + 3) = uVar10;
                    thunk_FUN_181dae2a0(puVar2,3,lVar14,iVar4);
                    thunk_FUN_181dae2a0(lVar14,local_88,iVar4);
                  }
                  else {
                    fVar23 = afStack_90[lVar21];
                    if (fVar23 < 0.01) goto LAB_1808b0ae3;
                    if (0.99 < fVar23) goto LAB_1808b0a38;
                    iVar16 = (int)plVar5[3];
                    lVar3 = *plVar5 + (longlong)iVar16 * 4;
                    uVar8 = iVar4 + iVar16;
                    if (lVar14 == 4) {
                      uVar10 = (uVar8 - (uVar8 & 0xf)) + 0x10;
                      if (((longlong)(int)uVar8 & 0xfU) == 0) {
                        uVar10 = uVar8;
                      }
                      *(uint *)(plVar5 + 3) = uVar10;
                      thunk_FUN_181dae2a0(puVar2,(longlong)(int)uVar8,lVar3);
                      thunk_FUN_181dae2a0(1.0 - fVar23,lVar3,iVar4);
                    }
                    else {
                      uVar19 = (ulonglong)(int)uVar8;
                      uVar17 = (uVar19 - (uVar8 & 0xf)) + 0x10;
                      if ((uVar19 & 0xf) == 0) {
                        uVar17 = uVar19;
                      }
                      *(int *)(plVar5 + 3) = (int)uVar17;
                      thunk_FUN_181dae2a0(puVar2,uVar17,lVar3,iVar4);
                    }
                    thunk_FUN_181dae2a0(lVar3,local_88,iVar4);
                  }
                  *(int *)(plVar5 + 3) = iVar16;
                  lVar14 = local_140;
                }
                else {
                  uVar13 = param_1[*(longlong *)(puVar20 + 2) + lVar21];
                  if (lVar14 == 3) {
                    thunk_FUN_181dae2a0();
                    lVar14 = local_140;
                  }
                  else {
LAB_1808b0a38:
                    thunk_FUN_181dae2a0(puVar2,uVar13,iVar4);
                    lVar14 = local_140;
                  }
                }
LAB_1808b0ae3:
                lVar21 = lVar21 + 1;
                local_148[0] = (float)((int)local_148[0] + 1);
                cVar7 = (char)param_8;
              } while ((int)local_148[0] < (int)puVar20[4]);
            }
          }
        }
        else {
          if (uVar10 == 2) {
            bVar22 = uVar8 != 9;
            goto LAB_1808b0995;
          }
          if (uVar10 == 3) {
            if (uVar8 - 9 < 2) goto LAB_1808b0b02;
            goto LAB_1808b0920;
          }
          if (uVar10 == 5) {
            if (uVar8 - 0x10 < 3) goto LAB_1808b0920;
          }
          else if (uVar10 == 4) {
            if (2 < uVar8 - 8) goto LAB_1808b0920;
          }
          else if (uVar10 == 6) {
            cVar7 = FUN_18061d160(4,uVar17);
            if ((cVar7 != '\0') && (cVar7 = FUN_18061d160(5,uVar17 & 0xffffffff), cVar7 == '\0')) {
              cVar7 = (char)param_8;
              goto LAB_1808b0920;
            }
          }
          else {
            if (uVar10 != 7) {
              bVar22 = uVar10 == uVar8;
              goto LAB_1808b0995;
            }
            if (uVar8 - 8 < 2) goto LAB_1808b0920;
          }
        }
LAB_1808b0b02:
        puVar20 = puVar20 + 6;
        cVar7 = (char)param_8;
      } while (puVar20 != local_138);
    }
  }
  FUN_181cbdb35(0x4024000000000000,0x4008000000000000);
  local_128[0] = *param_1;
  local_128[1] = param_1[1];
  local_108 = param_1[4];
  uStack_100 = param_1[5];
  local_128[2] = param_1[2];
  local_128[3] = param_1[3];
  local_e8 = param_1[8];
  uStack_e0 = param_1[9];
  local_f8 = param_1[6];
  uStack_f0 = param_1[7];
  local_c8 = param_1[0xc];
  uStack_c0 = param_1[0xd];
  pauVar1 = (undefined1 (*) [16])(param_1 + 0x10);
  local_d8 = param_1[10];
  uStack_d0 = param_1[0xb];
  local_b8 = param_1[0xe];
  uStack_b0 = param_1[0xf];
  local_a8 = *pauVar1;
  auVar6 = local_a8;
  if (0 < (int)*(uint *)*pauVar1) {
    local_a8._4_4_ = SUB124(*(undefined1 (*) [12])*pauVar1,4);
    uVar18 = local_a8._4_4_;
    puVar15 = local_128;
    uVar17 = (ulonglong)*(uint *)*pauVar1;
    local_a8 = auVar6;
    do {
      thunk_FUN_181dae2a0(*puVar15,uVar18);
      puVar15 = puVar15 + 1;
      uVar17 = uVar17 - 1;
    } while (uVar17 != 0);
  }
  *(undefined4 *)(plVar5 + 3) = local_98;
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

