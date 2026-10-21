
void analyst_entry_8abd70(longlong param_1,longlong *param_2)

{
  uint uVar1;
  undefined4 uVar2;
  longlong lVar3;
  longlong lVar4;
  longlong lVar5;
  longlong lVar6;
  longlong lVar7;
  longlong lVar8;
  longlong lVar9;
  longlong lVar10;
  longlong lVar11;
  longlong lVar12;
  longlong lVar13;
  longlong lVar14;
  longlong lVar15;
  longlong lVar16;
  longlong lVar17;
  longlong lVar18;
  longlong lVar19;
  int iVar20;
  undefined4 uVar21;
  undefined4 uVar22;
  uint uVar23;
  longlong *plVar24;
  undefined8 *puVar25;
  int iVar26;
  uint uVar27;
  ulonglong uVar28;
  ulonglong uVar29;
  float fVar30;
  undefined4 uVar31;
  undefined8 local_378 [16];
  undefined8 local_2f8;
  longlong lStack_2f0;
  undefined8 local_2e8 [16];
  uint local_268;
  int iStack_264;
  longlong lStack_260;
  longlong local_258 [4];
  undefined8 local_238;
  undefined8 uStack_230;
  undefined8 local_228;
  undefined8 uStack_220;
  undefined8 local_218;
  undefined8 uStack_210;
  undefined8 local_208;
  undefined8 uStack_200;
  undefined8 local_1f8;
  undefined8 uStack_1f0;
  undefined8 local_1e8;
  undefined8 uStack_1e0;
  uint local_1d8;
  int iStack_1d4;
  undefined4 uStack_1d0;
  undefined4 uStack_1cc;
  longlong local_1c8 [16];
  uint local_148;
  int iStack_144;
  undefined8 uStack_140;
  undefined8 local_138;
  undefined8 uStack_130;
  undefined8 local_128;
  undefined8 uStack_120;
  undefined8 local_118;
  undefined8 uStack_110;
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
  longlong lStack_b0;
  
                    /* 0x8abd70  1  analyst_entry_8abd70 */
  fVar30 = *(float *)(*(longlong *)(param_1 + 0x2c38) + 4);
  if (fVar30 <= 0.0) {
    fVar30 = 0.0;
  }
  if (1.0 <= fVar30) {
    fVar30 = 1.0;
  }
  func_0x000181cbdf57(fVar30 * 1.0986123 - 0.6931472);
  func_0x000181cbdf7b();
  uVar31 = func_0x000181cbdf57();
  uVar28 = 0;
  iStack_264 = *(int *)(param_1 + 0x740);
  iVar26 = 0;
  local_2e8[0] = 0;
  local_2e8[0xf] = 0;
  local_268 = *(uint *)(param_1 + 0x738);
  lStack_260 = (longlong)*(int *)(param_1 + 0x740) << 2;
  local_2e8[1] = 0;
  local_2e8[2] = 0;
  local_2e8[3] = 0;
  local_2e8[4] = 0;
  local_2e8[5] = 0;
  local_2e8[6] = 0;
  local_2e8[7] = 0;
  local_2e8[8] = 0;
  local_2e8[9] = 0;
  local_2e8[10] = 0;
  local_2e8[0xb] = 0;
  local_2e8[0xc] = 0;
  local_2e8[0xd] = 0;
  local_2e8[0xe] = 0;
  if (0 < (int)*(uint *)(param_1 + 0x738)) {
    puVar25 = local_2e8;
    do {
      iVar26 = iVar26 + 1;
      *puVar25 = *(undefined8 *)(((param_1 + 0x6b8) - (longlong)local_2e8) + (longlong)puVar25);
      puVar25 = puVar25 + 1;
    } while (iVar26 < (int)local_268);
  }
  local_378[0] = local_2e8[0];
  local_378[1] = local_2e8[1];
  local_378[2] = local_2e8[2];
  local_378[3] = local_2e8[3];
  local_378[4] = local_2e8[4];
  local_378[5] = local_2e8[5];
  local_378[6] = local_2e8[6];
  local_378[7] = local_2e8[7];
  local_378[8] = local_2e8[8];
  local_378[9] = local_2e8[9];
  local_378[10] = local_2e8[10];
  local_378[0xb] = local_2e8[0xb];
  local_378[0xc] = local_2e8[0xc];
  local_378[0xd] = local_2e8[0xd];
  local_2f8 = CONCAT44(iStack_264,local_268);
  local_378[0xe] = local_2e8[0xe];
  local_378[0xf] = local_2e8[0xf];
  lStack_2f0 = lStack_260;
  analyst_entry_8af9e0(local_378,uVar31);
  local_1d8 = *(uint *)(param_1 + 0x7c8);
  iStack_1d4 = *(int *)(param_1 + 2000);
  uVar1 = *(uint *)(param_2 + 3);
  if (0 < (int)local_1d8) {
    plVar24 = local_258;
    uVar29 = uVar28;
    do {
      uVar27 = (int)uVar29 + 1;
      uVar29 = (ulonglong)uVar27;
      *plVar24 = *(longlong *)(((param_1 + 0x748) - (longlong)local_258) + (longlong)plVar24);
      plVar24 = plVar24 + 1;
    } while ((int)uVar27 < (int)local_1d8);
  }
  local_1c8[0] = 0;
  local_148 = local_1d8;
  local_1c8[0xf] = 0;
  iStack_144 = iStack_1d4;
  uStack_140 = (longlong)iStack_1d4 << 2;
  local_1c8[1] = 0;
  local_1c8[2] = 0;
  local_1c8[3] = 0;
  local_1c8[4] = 0;
  local_1c8[5] = 0;
  local_1c8[6] = 0;
  local_1c8[7] = 0;
  local_1c8[8] = 0;
  local_1c8[9] = 0;
  local_1c8[10] = 0;
  local_1c8[0xb] = 0;
  local_1c8[0xc] = 0;
  local_1c8[0xd] = 0;
  local_1c8[0xe] = 0;
  uVar29 = uVar28;
  uVar27 = uVar1;
  if (0 < (int)local_1d8) {
    do {
      uVar23 = uVar27 + iStack_1d4;
      local_1c8[uVar29] = *param_2 + (longlong)(int)uVar27 * 4;
      uVar27 = (uVar23 - (uVar23 & 0xf)) + 0x10;
      if ((uVar23 & 0xf) == 0) {
        uVar27 = uVar23;
      }
      uVar29 = uVar29 + 1;
      *(uint *)(param_2 + 3) = uVar27;
    } while ((longlong)uVar29 < (longlong)(int)local_1d8);
  }
  iVar26 = 0;
  uVar31 = *(undefined4 *)(*(longlong *)(param_1 + 0x2c38) + 0x14);
  uVar2 = *(undefined4 *)(*(longlong *)(param_1 + 0x2c38) + 0x10);
  local_378[0] = 0;
  local_378[0xf] = 0;
  local_2f8 = CONCAT44(*(int *)(param_1 + 2000),*(int *)(param_1 + 0x7c8));
  lStack_2f0 = (longlong)*(int *)(param_1 + 2000) << 2;
  local_378[1] = 0;
  local_378[2] = 0;
  local_378[3] = 0;
  local_378[4] = 0;
  local_378[5] = 0;
  local_378[6] = 0;
  local_378[7] = 0;
  local_378[8] = 0;
  local_378[9] = 0;
  local_378[10] = 0;
  local_378[0xb] = 0;
  local_378[0xc] = 0;
  local_378[0xd] = 0;
  local_378[0xe] = 0;
  if (0 < *(int *)(param_1 + 0x7c8)) {
    puVar25 = local_378;
    do {
      iVar26 = iVar26 + 1;
      *puVar25 = *(undefined8 *)(((param_1 + 0x748) - (longlong)local_378) + (longlong)puVar25);
      puVar25 = puVar25 + 1;
    } while (iVar26 < (int)local_2f8);
  }
  iVar20 = iStack_144;
  uVar27 = local_148;
  lVar19 = local_1c8[0xf];
  lVar18 = local_1c8[0xe];
  lVar17 = local_1c8[0xd];
  lVar16 = local_1c8[0xc];
  lVar15 = local_1c8[0xb];
  lVar14 = local_1c8[10];
  lVar13 = local_1c8[9];
  lVar12 = local_1c8[8];
  lVar11 = local_1c8[7];
  lVar10 = local_1c8[6];
  lVar9 = local_1c8[5];
  lVar8 = local_1c8[4];
  lVar7 = local_1c8[3];
  lVar6 = local_1c8[2];
  lVar5 = local_1c8[1];
  lVar3 = local_1c8[0];
  uVar21 = (undefined4)uStack_140;
  uVar22 = uStack_140._4_4_;
  local_138 = local_378[0];
  uStack_130 = local_378[1];
  local_128 = local_378[2];
  uStack_120 = local_378[3];
  local_118 = local_378[4];
  uStack_110 = local_378[5];
  local_108 = local_378[6];
  uStack_100 = local_378[7];
  local_f8 = local_378[8];
  uStack_f0 = local_378[9];
  local_e8 = local_378[10];
  uStack_e0 = local_378[0xb];
  local_d8 = local_378[0xc];
  uStack_d0 = local_378[0xd];
  local_b8 = local_2f8;
  lStack_b0 = lStack_2f0;
  local_c8 = local_378[0xe];
  uStack_c0 = local_378[0xf];
  local_258[0] = local_1c8[0];
  local_258[1] = local_1c8[1];
  local_258[2] = local_1c8[2];
  local_258[3] = local_1c8[3];
  local_238 = local_1c8[4];
  uStack_230 = local_1c8[5];
  local_228 = local_1c8[6];
  uStack_220 = local_1c8[7];
  local_218 = local_1c8[8];
  uStack_210 = local_1c8[9];
  local_208 = local_1c8[10];
  uStack_200 = local_1c8[0xb];
  local_1f8 = local_1c8[0xc];
  uStack_1f0 = local_1c8[0xd];
  local_1e8 = local_1c8[0xe];
  uStack_1e0 = local_1c8[0xf];
  local_1d8 = local_148;
  iStack_1d4 = iStack_144;
  uStack_1d0 = (undefined4)uStack_140;
  uStack_1cc = uStack_140._4_4_;
  analyst_entry_8af5b0(local_258,&local_138,uVar2,uVar31,param_2);
  lVar4 = local_1c8[0];
  iVar26 = iStack_264;
  local_378[0] = local_2e8[0];
  local_378[1] = local_2e8[1];
  local_378[4] = local_2e8[4];
  local_378[5] = local_2e8[5];
  local_378[2] = local_2e8[2];
  local_378[3] = local_2e8[3];
  local_378[8] = local_2e8[8];
  local_378[9] = local_2e8[9];
  local_378[6] = local_2e8[6];
  local_378[7] = local_2e8[7];
  local_378[0xc] = local_2e8[0xc];
  local_378[0xd] = local_2e8[0xd];
  local_378[10] = local_2e8[10];
  local_378[0xb] = local_2e8[0xb];
  local_258[0] = lVar3;
  local_258[1] = lVar5;
  local_258[2] = lVar6;
  local_258[3] = lVar7;
  local_238 = lVar8;
  uStack_230 = lVar9;
  local_228 = lVar10;
  uStack_220 = lVar11;
  local_218 = lVar12;
  uStack_210 = lVar13;
  local_208 = lVar14;
  uStack_200 = lVar15;
  local_1f8 = lVar16;
  uStack_1f0 = lVar17;
  local_1e8 = lVar18;
  uStack_1e0 = lVar19;
  local_1d8 = uVar27;
  iStack_1d4 = iVar20;
  uStack_1d0 = uVar21;
  uStack_1cc = uVar22;
  local_2f8 = CONCAT44(iStack_264,local_268);
  lStack_2f0 = lStack_260;
  local_378[0xe] = local_2e8[0xe];
  local_378[0xf] = local_2e8[0xf];
  if (local_268 == uVar27) {
    if (0 < (int)local_268) {
      uVar29 = (ulonglong)local_268;
      do {
        func_0x000180001340(*(undefined8 *)((longlong)local_258 + uVar28),
                            *(undefined8 *)((longlong)local_378 + uVar28),iVar26);
        uVar28 = uVar28 + 8;
        uVar29 = uVar29 - 1;
      } while (uVar29 != 0);
    }
  }
  else if ((uVar27 == 1) && (0 < (int)local_268)) {
    puVar25 = local_378;
    uVar28 = (ulonglong)local_268;
    do {
      func_0x000180001340(lVar4,*puVar25,iVar26);
      puVar25 = puVar25 + 1;
      uVar28 = uVar28 - 1;
    } while (uVar28 != 0);
  }
  *(uint *)(param_2 + 3) = uVar1;
  return;
}

