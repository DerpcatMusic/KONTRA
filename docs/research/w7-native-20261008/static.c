/* Automated decompilation: inferred types; original bytes in static.asm are authoritative. */
/* VA 140d04400 */

void FUN_140d04400(longlong param_1,undefined8 param_2,short param_3)

{
  code *pcVar1;
  undefined1 uVar2;
  undefined2 uVar3;
  undefined4 uVar4;
  int iVar5;
  undefined1 auStack_28 [32];

  if (param_3 != 0x70) {
    FUN_1407fab50(auStack_28);
    _CxxThrowException(auStack_28,0x149e370d8);
    pcVar1 = (code *)swi(3);
    (*pcVar1)();
    return;
  }
  uVar4 = FUN_142a78270(param_2);
  switch(uVar4) {
  case 0:
    *(undefined4 *)(param_1 + 8) = 0;
    break;
  case 1:
    *(undefined4 *)(param_1 + 8) = 1;
    break;
  case 2:
    *(undefined4 *)(param_1 + 8) = 2;
    break;
  case 3:
    *(undefined4 *)(param_1 + 8) = 3;
    break;
  case 4:
    *(undefined4 *)(param_1 + 8) = 4;
    break;
  case 5:
    *(undefined4 *)(param_1 + 8) = 5;
    break;
  default:
    FUN_140a874c0(auStack_28);
    _CxxThrowException(auStack_28,0x149e37590);
    pcVar1 = (code *)swi(3);
    (*pcVar1)();
    return;
  }
  iVar5 = FUN_142a78270(param_2);
  if (iVar5 == 0) {
    *(undefined4 *)(param_1 + 0x28) = 0;
  }
  else if (iVar5 == 1) {
    *(undefined4 *)(param_1 + 0x28) = 1;
  }
  else {
    if (iVar5 != 2) {
      FUN_140a874c0(auStack_28);
      _CxxThrowException(auStack_28,0x149e37590);
      pcVar1 = (code *)swi(3);
      (*pcVar1)();
      return;
    }
    *(undefined4 *)(param_1 + 0x28) = 2;
  }
  uVar3 = FUN_142a78220(param_2);
  *(undefined2 *)(param_1 + 0xc) = uVar3;
  uVar3 = FUN_142a78220(param_2);
  *(undefined2 *)(param_1 + 0xe) = uVar3;
  uVar3 = FUN_142a78220(param_2);
  *(undefined2 *)(param_1 + 0x10) = uVar3;
  uVar3 = FUN_142a78220(param_2);
  *(undefined2 *)(param_1 + 0x12) = uVar3;
  uVar3 = FUN_142a78220(param_2);
  *(undefined2 *)(param_1 + 0x14) = uVar3;
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 0x24) = uVar4;
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 0x18) = uVar4;
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 0x1c) = uVar4;
  uVar2 = FUN_142a780c0(param_2);
  *(undefined1 *)(param_1 + 0x20) = uVar2;
  return;
}


/* VA 140d12ec0 */

void FUN_140d12ec0(longlong param_1,longlong param_2)

{
  int iVar1;
  code *pcVar2;
  undefined8 uVar3;
  undefined8 uVar4;
  undefined1 auStack_28 [32];

  uVar4 = 2;
  switch(*(undefined4 *)(param_1 + 8)) {
  case 0:
    uVar3 = 0;
    break;
  case 1:
    uVar3 = 1;
    break;
  case 2:
    uVar3 = 2;
    break;
  case 3:
    uVar3 = 3;
    break;
  case 4:
    uVar3 = 4;
    break;
  case 5:
    uVar3 = 5;
    break;
  default:
    FUN_140a874c0(auStack_28);
    _CxxThrowException(auStack_28,0x149e37590);
    pcVar2 = (code *)swi(3);
    (*pcVar2)();
    return;
  }
  FUN_142a7ddb0(param_2,uVar3);
  iVar1 = *(int *)(param_1 + 0x28);
  if (iVar1 == 0) {
    uVar4 = 0;
  }
  else if (iVar1 == 1) {
    uVar4 = 1;
  }
  else if (iVar1 != 2) {
    FUN_140a874c0(auStack_28);
    _CxxThrowException(auStack_28,0x149e37590);
    pcVar2 = (code *)swi(3);
    (*pcVar2)();
    return;
  }
  FUN_142a7ddb0(param_2,uVar4);
  FUN_142a7dd70(param_2,*(undefined2 *)(param_1 + 0xc));
  FUN_142a7dd70(param_2,*(undefined2 *)(param_1 + 0xe));
  FUN_142a7dd70(param_2,*(undefined2 *)(param_1 + 0x10));
  FUN_142a7dd70(param_2,*(undefined2 *)(param_1 + 0x12));
  FUN_142a7dd70(param_2,*(undefined2 *)(param_1 + 0x14));
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 0x24));
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 0x18));
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 0x1c));
  (**(code **)(**(longlong **)(param_2 + 8) + 0x18))(*(longlong **)(param_2 + 8),&stack0x00000010,1)
  ;
  return;
}


/* VA 140cff510 */

void FUN_140cff510(longlong param_1,undefined8 param_2,short param_3)

{
  code *pcVar1;
  undefined1 uVar2;
  int iVar3;
  undefined4 uVar4;
  undefined1 auStack_28 [32];

  if (param_3 != 0x60) {
    FUN_1407fab50(auStack_28);
    _CxxThrowException(auStack_28,0x149e370d8);
    pcVar1 = (code *)swi(3);
    (*pcVar1)();
    return;
  }
  iVar3 = FUN_142a78270(param_2);
  if (iVar3 == 0) {
    *(undefined4 *)(param_1 + 0x20) = 0;
  }
  else if (iVar3 == 1) {
    *(undefined4 *)(param_1 + 0x20) = 3;
  }
  else if (iVar3 == 2) {
    *(undefined4 *)(param_1 + 0x20) = 4;
  }
  else {
    if (iVar3 != 3) {
      FUN_140a874c0(auStack_28);
      _CxxThrowException(auStack_28,0x149e37590);
      pcVar1 = (code *)swi(3);
      (*pcVar1)();
      return;
    }
    *(undefined4 *)(param_1 + 0x20) = 1;
  }
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 8) = uVar4;
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 0xc) = uVar4;
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 0x18) = uVar4;
  uVar2 = FUN_142a780c0(param_2);
  *(undefined1 *)(param_1 + 0x1c) = uVar2;
  uVar4 = FUN_142a780f0(param_2);
  *(undefined4 *)(param_1 + 0x24) = uVar4;
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 0x28) = uVar4;
  return;
}


/* VA 140d0faa0 */

void FUN_140d0faa0(longlong param_1,longlong param_2)

{
  int iVar1;
  code *pcVar2;
  undefined8 uVar3;
  uint auStackX_10 [6];
  undefined1 auStack_28 [32];

  iVar1 = *(int *)(param_1 + 0x20);
  if (iVar1 == 0) {
    uVar3 = 0;
  }
  else if (iVar1 == 1) {
    uVar3 = 3;
  }
  else if (iVar1 == 3) {
    uVar3 = 1;
  }
  else {
    if (iVar1 != 4) {
      FUN_140a874c0(auStack_28);
      _CxxThrowException(auStack_28,0x149e37590);
      pcVar2 = (code *)swi(3);
      (*pcVar2)();
      return;
    }
    uVar3 = 2;
  }
  FUN_142a7ddb0(param_2,uVar3);
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 8));
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 0xc));
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 0x18));
  FUN_142a7dba0(param_2,*(undefined1 *)(param_1 + 0x1c));
  FUN_142a7dcd0(param_2,*(undefined4 *)(param_1 + 0x24));
  auStackX_10[0] = *(uint *)(param_1 + 0x28);
  if (*(int *)(param_2 + 0x10) != 1) {
    auStackX_10[0] =
         ((auStackX_10[0] & 0xffff) >> 8 | (uint)(ushort)((short)auStackX_10[0] << 8)) << 0x10 |
         auStackX_10[0] >> 0x18 | (uint)(ushort)((short)(auStackX_10[0] >> 0x10) << 8);
  }
  (**(code **)(**(longlong **)(param_2 + 8) + 0x18))(*(longlong **)(param_2 + 8),auStackX_10,4);
  return;
}


/* VA 140cfec70 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_140cfec70(longlong param_1,undefined8 param_2,short param_3,undefined8 param_4)

{
  code *pcVar1;
  undefined1 uVar2;
  byte bVar3;
  undefined2 uVar4;
  undefined4 uVar5;
  undefined8 *puVar6;
  longlong lVar7;
  ulonglong uVar8;
  undefined1 auStack_58 [8];
  undefined8 uStack_50;
  longlong alStack_38 [2];
  undefined1 auStack_28 [8];
  ulonglong uStack_20;
  ulonglong uStack_18;

  if (param_3 == 0x70) {
    uStack_50 = 0x140cfed27;
    FUN_140cf0360(param_2,param_1 + 0x28,param_4);
    uStack_50 = 0x140cfed2f;
    uVar2 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x31) = uVar2;
    uStack_50 = 0x140cfed3a;
    bVar3 = FUN_142a780c0(param_2);
    uStack_50 = 0x140cfed45;
    uVar4 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x32) = uVar4;
    uStack_50 = 0x140cfed51;
    uVar5 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x40) = uVar5;
    uStack_50 = 0x140cfed5c;
    uVar5 = FUN_142a780f0(param_2);
    *(undefined4 *)(param_1 + 0x50) = uVar5;
    uStack_50 = 0x140cfed69;
    uVar5 = FUN_142a780f0(param_2);
    *(undefined4 *)(param_1 + 0x54) = uVar5;
    uStack_50 = 0x140cfed7d;
    FUN_140cf0710(param_2,param_1 + 0x34,param_4);
    if (*(int *)(param_1 + 0x3c) < 0) {
      if (*(int *)(param_1 + 0x34) - 0x16U < 0x1fc) {
        *(undefined4 *)(param_1 + 0x44) = 1;
      }
      else if (*(int *)(param_1 + 0x34) - 0x214U < 0x7f) {
        *(uint *)(param_1 + 0x44) = (uint)bVar3;
      }
      else {
        *(undefined4 *)(param_1 + 0x44) = 0xffffffff;
      }
    }
    else {
      *(undefined4 *)(param_1 + 0x44) = 0;
    }
    return;
  }
  if (param_3 == 0x71) {
    uStack_50 = 0x140cfeca9;
    FUN_140cf0360(param_2,param_1 + 0x28,param_4);
    uStack_50 = 0x140cfecb1;
    uVar2 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x31) = uVar2;
    uStack_50 = 0x140cfecbc;
    uVar4 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x32) = uVar4;
    uStack_50 = 0x140cfecc8;
    uVar5 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x40) = uVar5;
    uStack_50 = 0x140cfecd3;
    uVar5 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x44) = uVar5;
    uStack_50 = 0x140cfecde;
    uVar5 = FUN_142a780f0(param_2);
    *(undefined4 *)(param_1 + 0x50) = uVar5;
    uStack_50 = 0x140cfeceb;
    uVar5 = FUN_142a780f0(param_2);
    *(undefined4 *)(param_1 + 0x54) = uVar5;
    uStack_18 = _DAT_14a2a58c0 ^ (ulonglong)auStack_58;
    puVar6 = (undefined8 *)FUN_142a78330(param_2,alStack_38,param_4);
    if (0xf < (ulonglong)puVar6[3]) {
      puVar6 = (undefined8 *)*puVar6;
    }
    uVar5 = FUN_14096af20(puVar6);
    *(undefined4 *)(param_1 + 0x34) = uVar5;
    if (0xf < uStack_20) {
      uVar8 = uStack_20 + 1;
      lVar7 = alStack_38[0];
      if (0xfff < uVar8) {
        uVar8 = uStack_20 + 0x28;
        lVar7 = *(longlong *)(alStack_38[0] + -8);
        if (0x1f < (alStack_38[0] - lVar7) - 8U) {
          _invalid_parameter_noinfo_noreturn();
          pcVar1 = (code *)swi(3);
          (*pcVar1)();
          return;
        }
      }
      free(lVar7,uVar8);
    }
    FUN_1443c9590(uStack_18 ^ (ulonglong)auStack_58);
    return;
  }
  uStack_50 = 0x140cfeddd;
  FUN_1407fab50(auStack_28);
  uStack_50 = 0x140cfedee;
  _CxxThrowException(auStack_28,0x149e370d8);
  pcVar1 = (code *)swi(3);
  (*pcVar1)();
  return;
}


/* VA 140d0f570 */

void FUN_140d0f570(longlong param_1,longlong param_2)

{
  int iVar1;
  code *pcVar2;
  longlong lVar3;
  undefined8 uVar4;
  ulonglong uVar5;
  undefined1 auStack_28 [32];

  iVar1 = *(int *)(param_1 + 0x28);
  if (iVar1 == 0) {
    uVar4 = 0;
  }
  else if (iVar1 == 1) {
    uVar4 = 1;
  }
  else {
    if (iVar1 != 2) {
      FUN_140a874c0(auStack_28);
      _CxxThrowException(auStack_28,0x149e37590);
      pcVar2 = (code *)swi(3);
      (*pcVar2)();
      return;
    }
    uVar4 = 2;
  }
  FUN_142a7ddb0(param_2,uVar4);
  FUN_142a7dba0(param_2,*(undefined1 *)(param_1 + 0x31));
  FUN_142a7dd70(param_2,*(undefined2 *)(param_1 + 0x32));
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 0x40));
  FUN_142a7ddb0(param_2,*(undefined4 *)(param_1 + 0x44));
  FUN_142a7dcd0(param_2,*(undefined4 *)(param_1 + 0x50));
  FUN_142a7dcd0(param_2,*(undefined4 *)(param_1 + 0x54));
  lVar3 = FUN_1408f5a00(*(undefined4 *)(param_1 + 0x34));
  uVar5 = 0xffffffffffffffff;
  do {
    uVar5 = uVar5 + 1;
  } while (*(char *)(lVar3 + uVar5) != '\0');
  (**(code **)(**(longlong **)(param_2 + 8) + 0x18))(*(longlong **)(param_2 + 8),&stack0x00000008,4)
  ;
  if ((int)uVar5 != 0) {
    (**(code **)(**(longlong **)(param_2 + 8) + 0x18))
              (*(longlong **)(param_2 + 8),lVar3,uVar5 & 0xffffffff);
  }
  return;
}


/* VA 140cf0360 */

void FUN_140cf0360(undefined8 param_1,undefined4 *param_2)

{
  code *pcVar1;
  int iVar2;
  undefined1 auStack_28 [32];

  iVar2 = FUN_142a78270();
  if (iVar2 == 0) {
    *param_2 = 0;
    return;
  }
  if (iVar2 != 1) {
    if (iVar2 == 2) {
      *param_2 = 2;
      return;
    }
    FUN_140a874c0(auStack_28);
    _CxxThrowException(auStack_28,0x149e37590);
    pcVar1 = (code *)swi(3);
    (*pcVar1)();
    return;
  }
  *param_2 = 1;
  return;
}


/* VA 140cf0710 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_140cf0710(undefined8 param_1,undefined4 *param_2)

{
  code *pcVar1;
  undefined4 uVar2;
  undefined8 *puVar3;
  longlong lVar4;
  ulonglong uVar5;
  undefined1 auStack_58 [32];
  longlong alStack_38 [3];
  ulonglong uStack_20;
  ulonglong uStack_18;

  uStack_18 = _DAT_14a2a58c0 ^ (ulonglong)auStack_58;
  puVar3 = (undefined8 *)FUN_142a78330(param_1,alStack_38);
  if (0xf < (ulonglong)puVar3[3]) {
    puVar3 = (undefined8 *)*puVar3;
  }
  uVar2 = FUN_14096af20(puVar3);
  *param_2 = uVar2;
  if (0xf < uStack_20) {
    uVar5 = uStack_20 + 1;
    lVar4 = alStack_38[0];
    if (0xfff < uVar5) {
      uVar5 = uStack_20 + 0x28;
      lVar4 = *(longlong *)(alStack_38[0] + -8);
      if (0x1f < (alStack_38[0] - lVar4) - 8U) {
        _invalid_parameter_noinfo_noreturn();
        pcVar1 = (code *)swi(3);
        (*pcVar1)();
        return;
      }
    }
    free(lVar4,uVar5);
  }
  FUN_1443c9590(uStack_18 ^ (ulonglong)auStack_58);
  return;
}


/* VA 14061c033 */

void FUN_14061c033(void)

{
  undefined4 uVar1;
  longlong lVar2;
  longlong *plVar3;
  undefined8 uVar4;
  longlong *unaff_RSI;
  longlong *plVar5;

  FUN_14051bb60();
  FUN_140513a90();
  lVar2 = FUN_141ef9200();
  unaff_RSI[0x82] = lVar2;
  uVar1 = (**(code **)(*unaff_RSI + 0x238))();
  *(undefined4 *)(lVar2 + 0x1148) = uVar1;
  if (*(longlong *)(lVar2 + 0x1140) != 0) {
    *(undefined4 *)(*(longlong *)(lVar2 + 0x1140) + 1000) = uVar1;
  }
  plVar3 = (longlong *)FUN_141ef9200();
  unaff_RSI[0x83] = (longlong)plVar3;
  plVar5 = unaff_RSI + 0x49;
  if (unaff_RSI == (longlong *)0x0) {
    plVar5 = (longlong *)0x0;
  }
  (**(code **)(*plVar3 + 0xe8))(plVar3,plVar5);
  uVar4 = FUN_140552340(unaff_RSI[0x83]);
  FUN_141ef65b0(uVar4,0x1447277b8,0x14470787b,0);
  FUN_141ef65b0(uVar4,0x1447277c8,0x1447277c0,1);
  FUN_141ef65b0(uVar4,0x1447277e8,0x1447277d8,2);
  FUN_141ef65b0(uVar4,0x144727810,0x144727800,3);
  FUN_141ef65b0(uVar4,0x144727828,0x14470b374,4);
  FUN_141ef65b0(uVar4,0x144727838,0x14470787b,5);
  plVar3 = (longlong *)FUN_141ef9200();
  unaff_RSI[0x84] = (longlong)plVar3;
  (**(code **)(*plVar3 + 0xe8))(plVar3,plVar5);
  uVar4 = FUN_140552340(unaff_RSI[0x84]);
  FUN_141ef65b0(uVar4,0x144727848,0x14470787b,0);
  FUN_141ef65b0(uVar4,0x14472784c,0x14470787b,2);
  FUN_141ef65b0(uVar4,0x144727850,0x14470787b,1);
  lVar2 = FUN_141ef9200();
  unaff_RSI[0x85] = lVar2;
  return;
}


/* VA 14091f1b0 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_14091f1b0(longlong param_1,undefined4 param_2,int param_3,longlong param_4)

{
  char cVar1;
  longlong *plVar2;
  longlong *plVar3;
  double dVar4;
  float fVar5;
  float fVar6;
  undefined2 uVar7;
  int iVar8;
  undefined4 uVar9;
  longlong lVar10;
  char *pcVar11;
  longlong lVar12;
  longlong lVar13;
  longlong lVar14;
  longlong *plVar15;
  uint uVar16;
  bool bVar18;
  char cVar19;
  float fVar20;
  undefined1 auStack_f8 [32];
  char cStack_d8;
  short asStack_d4 [2];
  ushort auStack_d0 [2];
  int iStack_cc;
  char acStack_b8 [128];
  ulonglong uStack_38;
  longlong lVar17;

  uStack_38 = _DAT_14a2a58c0 ^ (ulonglong)auStack_f8;
  lVar17 = 0;
  uVar16 = 0;
  plVar2 = *(longlong **)(param_4 + 8);
  plVar3 = *(longlong **)(param_4 + 0x10);
  *(undefined4 *)(param_1 + 0x38) = param_2;
  *(int *)(param_1 + 0x3c) = param_3;
  cStack_d8 = '\0';
  cVar19 = cStack_d8;
  fVar5 = _DAT_14470bad0;
  fVar6 = _DAT_144760108;
  iStack_cc = param_3;
joined_r0x00014091f203:
  _DAT_14470bad0 = fVar5;
  _DAT_144760108 = fVar6;
  cStack_d8 = cVar19;
  if (plVar2 != plVar3) {
    lVar14 = lVar17;
    do {
      lVar10 = lVar14 + 1;
      plVar15 = plVar2;
      if (*(char *)(*plVar2 + lVar14) != *(char *)(lVar14 + 0x144f0f600)) goto LAB_14091f304;
      lVar14 = lVar10;
    } while (lVar10 != 8);
    pcVar11 = (char *)plVar2[1];
    lVar14 = -(longlong)pcVar11;
    do {
      cVar1 = *pcVar11;
      pcVar11[(longlong)(acStack_b8 + lVar14)] = cVar1;
      pcVar11 = pcVar11 + 1;
    } while (cVar1 != '\0');
    plVar15 = plVar2 + 2;
    if (plVar15 != plVar3) {
      lVar14 = lVar17;
      do {
        lVar10 = lVar14 + 1;
        if (*(char *)(*plVar15 + lVar14) != *(char *)(lVar14 + 0x144f0f600)) goto LAB_14091f2c2;
        lVar14 = lVar10;
      } while (lVar10 != 8);
      pcVar11 = (char *)plVar2[3];
      lVar14 = -(longlong)pcVar11;
      do {
        cVar1 = *pcVar11;
        pcVar11[(longlong)(acStack_b8 + lVar14)] = cVar1;
        pcVar11 = pcVar11 + 1;
      } while (cVar1 != '\0');
      plVar15 = plVar2 + 4;
    }
LAB_14091f2c2:
    iVar8 = FUN_14076fed0(acStack_b8,0x144e76140,asStack_d4,auStack_d0);
    if ((iVar8 == 2) && (asStack_d4[0] != 0 || auStack_d0[0] != 0)) {
      *(ushort *)(param_1 + 0x5e) = asStack_d4[0] << 8 | auStack_d0[0];
    }
LAB_14091f304:
    if (plVar15 == plVar3) goto LAB_14091f573;
    lVar14 = *plVar15;
    iVar8 = strcmp(lVar14,0x144f0f608);
    if (iVar8 != 0) {
LAB_14091f39b:
      iVar8 = strcmp(lVar14,0x144f0f638);
      if (iVar8 == 0) {
        lVar14 = lVar17;
        do {
          lVar10 = lVar14;
          bVar18 = *(char *)(plVar15[1] + lVar10) == *(char *)(lVar10 + 0x144f0f648);
          if (!bVar18) goto LAB_14091f3d7;
          lVar14 = lVar10 + 1;
        } while (lVar10 + 1 != 4);
        bVar18 = *(char *)(plVar15[1] + lVar10) == *(char *)(lVar10 + 0x144f0f648);
LAB_14091f3d7:
        plVar15 = plVar15 + 2;
        *(bool *)(param_1 + 0x31) = bVar18;
      }
      if (plVar15 != plVar3) {
        iVar8 = strcmp(*plVar15,0x144f0f650);
        if (iVar8 == 0) {
          lVar14 = lVar17;
          do {
            lVar10 = lVar14;
            cVar19 = *(char *)(plVar15[1] + lVar10) == *(char *)(lVar10 + 0x144f0f648);
            if (!(bool)cVar19) goto LAB_14091f427;
            lVar14 = lVar10 + 1;
          } while (lVar10 + 1 != 4);
          cVar19 = *(char *)(plVar15[1] + lVar10) == *(char *)(lVar10 + 0x144f0f648);
LAB_14091f427:
          plVar15 = plVar15 + 2;
          cStack_d8 = cVar19;
          if (plVar15 == plVar3) goto LAB_14091f573;
        }
        lVar14 = lVar17;
        do {
          lVar10 = lVar14 + 1;
          if (*(char *)(*plVar15 + lVar14) != *(char *)(lVar14 + 0x144f0f670)) goto LAB_14091f483;
          lVar14 = lVar10;
        } while (lVar10 != 7);
        uVar7 = atoi(plVar15[1]);
        plVar15 = plVar15 + 2;
        *(undefined2 *)(param_1 + 0x32) = uVar7;
        if (plVar15 != plVar3) {
LAB_14091f483:
          lVar14 = lVar17;
          do {
            lVar10 = lVar14 + 1;
            if (*(char *)(*plVar15 + lVar14) != *(char *)(lVar14 + 0x144f0f678)) goto LAB_14091f4b8;
            lVar14 = lVar10;
          } while (lVar10 != 7);
          uVar9 = FUN_14096af20(plVar15[1]);
          *(undefined4 *)(param_1 + 0x34) = uVar9;
          plVar15 = plVar15 + 2;
LAB_14091f4b8:
          if (plVar15 != plVar3) {
            lVar14 = lVar17;
            do {
              lVar10 = lVar14 + 1;
              if (*(char *)(*plVar15 + lVar14) != *(char *)(lVar14 + 0x144f0f680))
              goto LAB_14091f4fe;
              lVar14 = lVar10;
            } while (lVar10 != 7);
            uVar9 = atoi(plVar15[1]);
            plVar15 = plVar15 + 2;
            *(undefined4 *)(param_1 + 0x40) = uVar9;
            if (plVar15 != plVar3) {
LAB_14091f4fe:
              iVar8 = strcmp(*plVar15,0x144f0f688);
              if (iVar8 == 0) {
                dVar4 = (double)atof(plVar15[1]);
                plVar15 = plVar15 + 2;
                *(float *)(param_1 + 0x50) = (float)dVar4;
              }
              if (plVar15 != plVar3) {
                lVar14 = lVar17;
                do {
                  lVar10 = lVar14 + 1;
                  if (*(char *)(*plVar15 + lVar14) != *(char *)(lVar14 + 0x144f0f698))
                  goto LAB_14091f573;
                  lVar14 = lVar10;
                } while (lVar10 != 8);
                dVar4 = (double)atof(plVar15[1]);
                plVar15 = plVar15 + 2;
                *(float *)(param_1 + 0x54) = (float)dVar4;
              }
            }
          }
        }
      }
      goto LAB_14091f573;
    }
    lVar10 = plVar15[1];
    lVar12 = lVar17;
    do {
      lVar13 = lVar12 + 1;
      if (*(char *)(lVar10 + lVar12) != (&DAT_144e783b0)[lVar12]) {
        iVar8 = strcmp(lVar10,0x144f0f618);
        if (iVar8 == 0) {
          *(undefined4 *)(param_1 + 0x28) = 1;
          cVar1 = cStack_d8;
        }
        else {
          iVar8 = strcmp(lVar10,0x144f0f628);
          cVar19 = cStack_d8;
          if (iVar8 != 0) goto LAB_14091f39b;
          *(undefined4 *)(param_1 + 0x28) = 2;
          cVar1 = cStack_d8;
        }
        goto LAB_14091f57d;
      }
      lVar12 = lVar13;
    } while (lVar13 != 8);
    *(undefined4 *)(param_1 + 0x28) = 0;
    cVar1 = cStack_d8;
    goto LAB_14091f57d;
  }
  if ((*(ushort *)(param_1 + 0x5e) < 0x70) && (*(int *)(param_1 + 0x34) == 0x296)) {
    fVar20 = *(float *)(param_1 + 0x50);
    if (fVar20 < fVar5) {
      fVar20 = fVar5 - (fVar5 - fVar20) * fVar6;
    }
    else {
      fVar20 = (fVar20 - fVar5) * fVar6 + fVar5;
    }
    *(float *)(param_1 + 0x50) = fVar20;
    fVar20 = *(float *)(param_1 + 0x54);
    if (fVar20 < fVar5) {
      *(float *)(param_1 + 0x54) = fVar5 - (fVar5 - fVar20) * fVar6;
    }
    else {
      *(float *)(param_1 + 0x54) = (fVar20 - fVar5) * fVar6 + fVar5;
    }
  }
  if (iStack_cc < 0) {
    if (*(int *)(param_1 + 0x34) - 0x16U < 0x1fc) {
      *(undefined4 *)(param_1 + 0x44) = 1;
      goto LAB_14091f67f;
    }
    if (0x7e < *(int *)(param_1 + 0x34) - 0x214U) {
      *(undefined4 *)(param_1 + 0x44) = 0xffffffff;
      goto LAB_14091f67f;
    }
    uVar16 = (uint)(cVar19 != '\0');
  }
  *(uint *)(param_1 + 0x44) = uVar16;
LAB_14091f67f:
  FUN_1443c9590(uStack_38 ^ (ulonglong)auStack_f8);
  return;
LAB_14091f573:
  bVar18 = plVar15 == plVar2;
  cVar1 = cVar19;
  plVar2 = plVar15;
  cVar19 = cStack_d8;
  fVar5 = _DAT_14470bad0;
  fVar6 = _DAT_144760108;
  if (bVar18) {
LAB_14091f57d:
    cStack_d8 = cVar1;
    plVar2 = plVar15 + 2;
    cVar19 = cStack_d8;
    fVar5 = _DAT_14470bad0;
    fVar6 = _DAT_144760108;
  }
  goto joined_r0x00014091f203;
}


/* VA 14096e6b0 */

void FUN_14096e6b0(longlong param_1,int param_2)

{
  longlong *plVar1;
  longlong lVar2;
  undefined8 *puVar3;
  code *pcVar4;
  int iVar5;
  int iVar6;
  int iVar7;
  longlong lVar8;
  longlong lVar9;
  uint uVar10;
  longlong lVar11;
  longlong *plVar12;
  longlong lVar13;
  ulonglong uVar14;
  undefined8 *puVar15;
  uint uVar16;
  longlong *plVar17;
  ulonglong uVar18;
  ulonglong uVar19;
  int iVar20;
  int aiStackX_10 [2];
  longlong lStackX_18;
  longlong lStackX_20;

  iVar5 = param_2 + 1;
  if (param_2 < 0) {
    iVar5 = 0x40;
  }
  iVar20 = 0;
  if (-1 < param_2) {
    iVar20 = param_2;
  }
  if (iVar20 < iVar5) {
    lVar13 = (longlong)iVar20;
    iVar20 = iVar20 << 7;
    lStackX_20 = lVar13 * 8 + 0x18;
    do {
      uVar19 = 0;
      lVar2 = *(longlong *)(lStackX_20 + *(longlong *)(param_1 + 0x2d7f8));
      aiStackX_10[0] = iVar20;
      lStackX_18 = lVar13;
      iVar6 = _Mtx_lock(lVar2 + 0x4a8);
      if (iVar6 != 0) {
        __Throw_Cpp_error_std__YAXH_Z(5);
        pcVar4 = (code *)swi(3);
        (*pcVar4)();
        return;
      }
      if (*(int *)(lVar2 + 0x4f4) == 0x7fffffff) {
        *(undefined4 *)(lVar2 + 0x4f4) = 0x7ffffffe;
        __Throw_Cpp_error_std__YAXH_Z(6);
        pcVar4 = (code *)swi(3);
        (*pcVar4)();
        return;
      }
      lVar11 = 8;
      plVar12 = (longlong *)(lVar2 + 0x520);
      do {
        plVar1 = plVar12 + -4;
        plVar12[-3] = (longlong)plVar1;
        *plVar1 = (longlong)plVar1;
        plVar1 = plVar12 + -1;
        plVar12[-5] = 0;
        *plVar1 = (longlong)plVar1;
        *plVar12 = (longlong)plVar1;
        plVar1 = plVar12 + 2;
        plVar12[-2] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[3] = (longlong)plVar1;
        plVar1 = plVar12 + 5;
        plVar12[1] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[6] = (longlong)plVar1;
        plVar1 = plVar12 + 8;
        plVar12[4] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[9] = (longlong)plVar1;
        plVar1 = plVar12 + 0xb;
        plVar12[7] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0xc] = (longlong)plVar1;
        plVar1 = plVar12 + 0xe;
        plVar12[10] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0xf] = (longlong)plVar1;
        plVar1 = plVar12 + 0x11;
        plVar12[0xd] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x12] = (longlong)plVar1;
        plVar1 = plVar12 + 0x14;
        plVar12[0x10] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x15] = (longlong)plVar1;
        plVar1 = plVar12 + 0x17;
        plVar12[0x13] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x18] = (longlong)plVar1;
        plVar1 = plVar12 + 0x1a;
        plVar12[0x16] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x1b] = (longlong)plVar1;
        plVar1 = plVar12 + 0x1d;
        plVar12[0x19] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x1e] = (longlong)plVar1;
        plVar1 = plVar12 + 0x20;
        plVar12[0x1c] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x21] = (longlong)plVar1;
        plVar1 = plVar12 + 0x23;
        plVar12[0x1f] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x24] = (longlong)plVar1;
        plVar1 = plVar12 + 0x26;
        plVar12[0x22] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x27] = (longlong)plVar1;
        plVar1 = plVar12 + 0x29;
        plVar12[0x25] = 0;
        *plVar1 = (longlong)plVar1;
        plVar12[0x2a] = (longlong)plVar1;
        plVar12[0x28] = 0;
        lVar11 = lVar11 + -1;
        plVar12 = plVar12 + 0x30;
      } while (lVar11 != 0);
      lVar11 = lVar2 + 0x1100;
      lVar9 = 0x801;
      do {
        *(longlong *)lVar11 = lVar11;
        *(longlong *)(lVar11 + 8) = lVar11;
        *(undefined8 *)(lVar11 + -8) = 0;
        lVar11 = lVar11 + 0x18;
        lVar9 = lVar9 + -1;
      } while (lVar9 != 0);
      if (*(int *)(lVar2 + 0xe7c8) == -1) {
        uVar16 = 0x60;
        if (0 < (int)*(uint *)(lVar2 + 0xe7d4)) {
          uVar16 = *(uint *)(lVar2 + 0xe7d4);
        }
        uVar10 = *(uint *)(lVar2 + 0xe7b0) / uVar16;
        plVar12 = (longlong *)((ulonglong)*(uint *)(lVar2 + 0xe7b0) % (ulonglong)uVar16);
      }
      else {
        uVar10 = *(uint *)(lVar2 + 0xe7cc);
        plVar12 = (longlong *)0x0;
      }
      uVar14 = uVar19;
      if (0 < (int)uVar10) {
        do {
          iVar6 = 0x60;
          if (0 < *(int *)(lVar2 + 0xe7d4)) {
            iVar6 = *(int *)(lVar2 + 0xe7d4);
          }
          lVar11 = (longlong)(iVar6 * (int)uVar14) + *(longlong *)(lVar2 + 0xe7a8);
          if (*(int *)(lVar11 + 0x28) == 1) {
            lVar9 = (ulonglong)*(ushort *)(lVar11 + 0x32) + 0x35;
          }
          else {
            if (*(int *)(lVar11 + 0x28) != 2) {
LAB_14096ec40:
              _Mtx_unlock(lVar2 + 0x4a8);
              return;
            }
            lVar9 = (ulonglong)*(ushort *)(lVar11 + 0x32) + 0xb5;
          }
          plVar12 = (longlong *)(lVar11 + 0x18);
          plVar1 = (longlong *)(lVar2 + lVar9 * 0x18);
          uVar16 = (int)uVar14 + 1;
          uVar14 = (ulonglong)uVar16;
          puVar3 = (undefined8 *)plVar1[2];
          *(undefined8 **)(lVar11 + 0x20) = puVar3;
          *plVar12 = (longlong)(plVar1 + 1);
          plVar1[2] = (longlong)plVar12;
          *puVar3 = plVar12;
          *plVar1 = *plVar1 + 1;
        } while ((int)uVar16 < (int)uVar10);
      }
      if (*(int *)(lVar2 + 0xe800) == 0) {
        iVar6 = 0;
      }
      else {
        iVar6 = (int)(*(longlong *)(lVar2 + 0x420) - *(longlong *)(lVar2 + 0x418) >> 1);
      }
      if (0 < iVar6) {
        do {
          uVar14 = 0;
          uVar16 = *(short *)(*(longlong *)(lVar2 + 0x418) + uVar19 * 2) + iVar20;
          if ((int)uVar16 < 0) {
            iVar7 = -1;
          }
          else {
            iVar7 = (int)(uVar16 + ((int)uVar16 >> 0x1f & 0x7fU)) >> 7;
          }
          plVar12 = (longlong *)(longlong)iVar7;
          uVar16 = uVar16 & 0x8000007f;
          if ((int)uVar16 < 0) {
            uVar16 = (uVar16 - 1 | 0xffffff80) + 1;
          }
          lVar13 = *(longlong *)
                    (*(longlong *)(*(longlong *)(param_1 + 0x2d7f8) + 0x18 + (longlong)plVar12 * 8)
                     + 0x18 + (longlong)(int)uVar16 * 8);
          if (*(int *)(lVar13 + 0x1cf08) == -1) {
            uVar16 = 0x60;
            if (0 < (int)*(uint *)(lVar13 + 0x1cf14)) {
              uVar16 = *(uint *)(lVar13 + 0x1cf14);
            }
            uVar10 = *(uint *)(lVar13 + 0x1cef0) / uVar16;
            plVar12 = (longlong *)((ulonglong)*(uint *)(lVar13 + 0x1cef0) % (ulonglong)uVar16);
          }
          else {
            uVar10 = *(uint *)(lVar13 + 0x1cf0c);
          }
          uVar18 = uVar14;
          if (0 < (int)uVar10) {
            do {
              iVar7 = 0x60;
              if (0 < *(int *)(lVar13 + 0x1cf14)) {
                iVar7 = *(int *)(lVar13 + 0x1cf14);
              }
              lVar11 = (longlong)(iVar7 * (int)uVar18) + *(longlong *)(lVar13 + 0x1cee8);
              if (*(int *)(lVar11 + 0x28) == 1) {
                lVar9 = (ulonglong)*(ushort *)(lVar11 + 0x32) + 0x35;
              }
              else {
                if (*(int *)(lVar11 + 0x28) != 2) goto LAB_14096ec40;
                lVar9 = (ulonglong)*(ushort *)(lVar11 + 0x32) + 0xb5;
              }
              puVar15 = (undefined8 *)(lVar11 + 0x18);
              plVar1 = (longlong *)(lVar2 + lVar9 * 0x18);
              uVar16 = (int)uVar18 + 1;
              puVar3 = (undefined8 *)plVar1[2];
              plVar12 = plVar1 + 1;
              *(undefined8 **)(lVar11 + 0x20) = puVar3;
              *puVar15 = plVar12;
              plVar1[2] = (longlong)puVar15;
              *puVar3 = puVar15;
              *plVar1 = *plVar1 + 1;
              uVar18 = (ulonglong)uVar16;
            } while ((int)uVar16 < (int)uVar10);
          }
          iVar7 = (int)(*(longlong *)(lVar13 + 0x11378) - *(longlong *)(lVar13 + 0x11370) >> 3);
          if (0 < iVar7) {
            do {
              lVar11 = *(longlong *)(*(longlong *)(lVar13 + 0x11370) + uVar14 * 8);
              if (*(int *)(lVar11 + 0x108) == -1) {
                uVar16 = 0x60;
                if (0 < (int)*(uint *)(lVar11 + 0x114)) {
                  uVar16 = *(uint *)(lVar11 + 0x114);
                }
                uVar16 = *(uint *)(lVar11 + 0xf0) / uVar16;
              }
              else {
                uVar16 = *(uint *)(lVar11 + 0x10c);
              }
              plVar12 = (longlong *)0x0;
              if (0 < (int)uVar16) {
                do {
                  iVar20 = 0x60;
                  if (0 < *(int *)(lVar11 + 0x114)) {
                    iVar20 = *(int *)(lVar11 + 0x114);
                  }
                  lVar9 = (longlong)(iVar20 * (int)plVar12) + *(longlong *)(lVar11 + 0xe8);
                  if (*(int *)(lVar9 + 0x28) == 1) {
                    lVar8 = (ulonglong)*(ushort *)(lVar9 + 0x32) + 0x35;
                  }
                  else {
                    if (*(int *)(lVar9 + 0x28) != 2) goto LAB_14096ec40;
                    lVar8 = (ulonglong)*(ushort *)(lVar9 + 0x32) + 0xb5;
                  }
                  plVar17 = (longlong *)(lVar9 + 0x18);
                  plVar1 = (longlong *)(lVar2 + lVar8 * 0x18);
                  uVar10 = (int)plVar12 + 1;
                  plVar12 = (longlong *)(ulonglong)uVar10;
                  puVar3 = (undefined8 *)plVar1[2];
                  *(undefined8 **)(lVar9 + 0x20) = puVar3;
                  *plVar17 = (longlong)(plVar1 + 1);
                  plVar1[2] = (longlong)plVar17;
                  *puVar3 = plVar17;
                  *plVar1 = *plVar1 + 1;
                } while ((int)uVar10 < (int)uVar16);
              }
              uVar14 = uVar14 + 1;
              iVar20 = aiStackX_10[0];
            } while ((longlong)uVar14 < (longlong)iVar7);
          }
          uVar19 = uVar19 + 1;
          lVar13 = lStackX_18;
        } while ((longlong)uVar19 < (longlong)iVar6);
      }
      _Mtx_unlock(lVar2 + 0x4a8,plVar12);
      iVar20 = iVar20 + 0x80;
      lStackX_20 = lStackX_20 + 8;
      lVar13 = lVar13 + 1;
      lStackX_18 = lVar13;
    } while (lVar13 < iVar5);
  }
  aiStackX_10[0] = 0;
  FUN_14070df50(*(longlong *)(param_1 + 0x11bf8) + 0x1200,aiStackX_10);
  return;
}


/* VA 140958d50 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_140958d50(longlong param_1,float param_2,uint param_3,int param_4,int param_5,int param_6,
                  int param_7,undefined4 param_8,undefined4 param_9)

{
  longlong *plVar1;
  code *pcVar2;
  bool bVar3;
  float fVar4;
  int iVar5;
  int iVar6;
  uint uVar7;
  longlong lVar8;
  uint uVar9;
  uint *puVar10;
  int *piVar11;
  int iVar12;
  uint *puVar13;
  longlong *plVar14;
  uint *puVar15;
  float fVar16;
  float fVar17;
  undefined8 in_stack_ffffffffffffff58;
  undefined4 uVar18;
  undefined4 auStack_78 [2];
  undefined4 auStack_70 [2];
  float afStack_68 [16];

  iVar6 = param_7;
  uVar18 = (undefined4)((ulonglong)in_stack_ffffffffffffff58 >> 0x20);
  piVar11 = *(int **)(param_1 + 0x11c18);
  iVar12 = 0;
  if ((*piVar11 != 0) && (-1 < piVar11[1])) {
    lVar8 = _Xtime_get_ticks();
    if ((lVar8 - *(longlong *)(piVar11 + 4) == 5000000) ||
       (lVar8 - *(longlong *)(piVar11 + 4) < 5000000)) {
      FUN_1408f3400(param_1,param_3,param_4,param_5,CONCAT44(uVar18,param_6),iVar6,1,
                    *(undefined2 *)(*(longlong *)(param_1 + 0x11c18) + 4),1);
      piVar11 = *(int **)(param_1 + 0x11c18);
      if (*piVar11 == 1) {
        *piVar11 = 0;
      }
      piVar11[1] = -1;
      LOCK();
      *(undefined1 *)(param_1 + 0x120a1) = 1;
      UNLOCK();
    }
  }
  iVar5 = _DAT_144fd4c80;
  fVar4 = _DAT_14470bad4;
  if (iVar6 - 0x29fU < 7) {
    uVar7 = param_3 >> 7;
    if ((int)param_3 < 0) {
      uVar7 = 0xffffffff;
    }
    lVar8 = *(longlong *)(*(longlong *)(param_1 + 0x2d7f8) + 0x18 + (longlong)(int)uVar7 * 8);
    plVar14 = (longlong *)(lVar8 + 0xe7a8);
    puVar13 = (uint *)(lVar8 + 0xe7b0);
    puVar15 = (uint *)(lVar8 + 0xe7d4);
    puVar10 = (uint *)(lVar8 + 0xe7cc);
    piVar11 = (int *)(lVar8 + 0xe7c8);
  }
  else {
    uVar7 = param_3 >> 7;
    if ((int)param_3 < 0) {
      uVar7 = 0xffffffff;
    }
    param_3 = param_3 & 0x8000007f;
    if ((int)param_3 < 0) {
      param_3 = (param_3 - 1 | 0xffffff80) + 1;
    }
    lVar8 = *(longlong *)
             (*(longlong *)(*(longlong *)(param_1 + 0x2d7f8) + 0x18 + (longlong)(int)uVar7 * 8) +
              0x18 + (longlong)(int)param_3 * 8);
    if (param_4 < 0) {
      plVar14 = (longlong *)(lVar8 + 0x1cee8);
      puVar13 = (uint *)(lVar8 + 0x1cef0);
      puVar15 = (uint *)(lVar8 + 0x1cf14);
      puVar10 = (uint *)(lVar8 + 0x1cf0c);
      piVar11 = (int *)(lVar8 + 0x1cf08);
    }
    else {
      lVar8 = *(longlong *)(*(longlong *)(lVar8 + 0x11370) + (longlong)param_4 * 8);
      plVar14 = (longlong *)(lVar8 + 0xe8);
      puVar13 = (uint *)(lVar8 + 0xf0);
      puVar15 = (uint *)(lVar8 + 0x114);
      puVar10 = (uint *)(lVar8 + 0x10c);
      piVar11 = (int *)(lVar8 + 0x108);
    }
  }
  bVar3 = false;
  if (*piVar11 == -1) {
    uVar7 = 0x60;
    if (0 < (int)*puVar15) {
      uVar7 = *puVar15;
    }
    uVar7 = *puVar13 / uVar7;
  }
  else {
    uVar7 = *puVar10;
  }
  if (0 < (int)uVar7) {
    do {
      uVar9 = 0x60;
      if (0 < (int)*puVar15) {
        uVar9 = *puVar15;
      }
      lVar8 = (longlong)(int)(uVar9 * iVar12) + *plVar14;
      if (((((*(int *)(lVar8 + 0x28) != 2) || ((int)(uint)*(ushort *)(lVar8 + 0x32) < iVar5)) &&
           ((*(int *)(lVar8 + 0x40) < 0 || (*(int *)(lVar8 + 0x40) == param_5)))) &&
          (*(int *)(lVar8 + 0x34) == iVar6)) &&
         ((-1 < *(int *)(lVar8 + 0x3c) || (*(int *)(lVar8 + 0x44) == param_6)))) {
        fVar17 = param_2;
        if (*(int *)(lVar8 + 0x58) < 1) {
          fVar17 = (param_2 - *(float *)(lVar8 + 0x50)) /
                   (*(float *)(lVar8 + 0x54) - *(float *)(lVar8 + 0x50));
        }
        fVar16 = 0.0;
        if (0.0 <= fVar17) {
          fVar16 = fVar17;
        }
        fVar17 = fVar4;
        if (fVar16 <= fVar4) {
          fVar17 = fVar16;
        }
        FUN_140968460(lVar8);
        if ((*(int *)(lVar8 + 0x28) == 2) && (!bVar3)) {
          bVar3 = true;
          auStack_78[0] = param_9;
          *(float *)(lVar8 + 0x2c) = fVar17;
          plVar1 = *(longlong **)(param_1 + 0x2d878);
          auStack_70[0] = param_8;
          param_7 = CONCAT22(param_7._2_2_,*(undefined2 *)(lVar8 + 0x32));
          afStack_68[0] = fVar17;
          if (plVar1 == (longlong *)0x0) {
            __Xbad_function_call_std__YAXXZ();
            pcVar2 = (code *)swi(3);
            (*pcVar2)();
            return;
          }
          (**(code **)(*plVar1 + 0x10))(plVar1,&param_7,afStack_68,auStack_70,auStack_78);
        }
      }
      iVar12 = iVar12 + 1;
    } while (iVar12 < (int)uVar7);
  }
  return;
}


/* VA 140d0a2c0 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_140d0a2c0(longlong param_1,undefined8 param_2,ushort param_3,undefined8 param_4)

{
  undefined1 *puVar1;
  code *pcVar2;
  undefined1 uVar3;
  char cVar4;
  undefined2 uVar5;
  undefined4 uVar6;
  undefined4 uVar7;
  undefined4 uVar8;
  undefined8 uVar9;
  undefined1 auStack_1a8 [32];
  int aiStack_188 [2];
  undefined1 auStack_180 [32];
  undefined1 auStack_160 [288];
  ulonglong uStack_40;

  uStack_40 = _DAT_14a2a58c0 ^ (ulonglong)auStack_1a8;
  uVar6 = FUN_140ce8190(*(undefined8 *)(param_1 + 0x11318),0);
  uVar7 = FUN_140ce8190(*(undefined8 *)(param_1 + 0x11318),1);
  switch(param_3) {
  case 0x80:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cfb6d0(param_2,param_1 + 0x10db0,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c0a8,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c168,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c228,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c2e8,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c3a8,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    break;
  default:
    FUN_1407fab50(auStack_180);
    _CxxThrowException(auStack_180,0x149e370d8);
    pcVar2 = (code *)swi(3);
    (*pcVar2)();
    return;
  case 0x82:
  case 0x90:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cfb6d0(param_2,param_1 + 0x10db0,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c0a8,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c168,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c228,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c2e8,param_4);
    FUN_14093d6c0(param_2,param_1 + 0x1c3a8,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    goto code_r0x000140d0a831;
  case 0x91:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
    break;
  case 0x92:
  case 0xa0:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
code_r0x000140d0a831:
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    break;
  case 0xa1:
  case 0xa2:
  case 0xa3:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar6 = FUN_142a78270(param_2);
    uVar7 = FUN_142a78270(param_2);
    break;
  case 0xa4:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12044) = uVar8;
    break;
  case 0xa5:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar6 = FUN_142a78270(param_2);
    uVar7 = FUN_142a78270(param_2);
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12044) = uVar8;
    break;
  case 0xa6:
  case 0xa7:
  case 0xa8:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cefcf0(param_2,param_1 + 0x1ced0,param_4);
    goto code_r0x000140d0b61d;
  case 0xa9:
  case 0xab:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cf14f0(param_2,param_1 + 0x1cf08,param_4);
code_r0x000140d0b61d:
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar6 = FUN_142a78270(param_2);
    uVar7 = FUN_142a78270(param_2);
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12044) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef9) = uVar3;
    break;
  case 0xaa:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cf14f0(param_2,param_1 + 0x1cf08,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar6 = FUN_142a78270(param_2);
    uVar7 = FUN_142a78270(param_2);
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12044) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef9) = uVar3;
    goto code_r0x000140d0c3fb;
  case 0xac:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cf14f0(param_2,param_1 + 0x1cf08,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar6 = FUN_142a78270(param_2);
    uVar7 = FUN_142a78270(param_2);
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12044) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef9) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1cf20) = uVar8;
    break;
  case 0xad:
  case 0xae:
  case 0xaf:
  case 0xb0:
  case 0xb1:
  case 0xb2:
  case 0xb3:
  case 0xb4:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cf14f0(param_2,param_1 + 0x1cf08,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar6 = FUN_142a78270(param_2);
    uVar7 = FUN_142a78270(param_2);
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12044) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef9) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1cf20) = uVar8;
    FUN_140cfb6d0(param_2,param_1 + 0x1bee8,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0b) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bde) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be8) = uVar8;
    break;
  case 0xb5:
    FUN_140cf03c0(param_2,param_1 + 0x21b88,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb15) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb16) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1fb18) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x1fb14) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bd8) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdc) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be0) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bdd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfc) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfe) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf9) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfa) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfb) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c03) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c04) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c05) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c06) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c07) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bfd) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c01) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c09) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0a) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0c) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x113c8) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12038) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1203c) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c10) = uVar3;
    uVar5 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x11c0e) = uVar5;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x12040) = uVar3;
    FUN_140cf0ad0(param_2,param_1 + 0x1cf28,param_4);
    FUN_140cf0dc0(param_2,param_1 + 0x1cf40,param_4);
    FUN_140cf14f0(param_2,param_1 + 0x1cf08,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bec) = uVar3;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf0) = uVar7;
    uVar7 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11bf4) = uVar7;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bf8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c08) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x21b7f) = uVar3;
    uVar6 = FUN_142a78270(param_2);
    uVar7 = FUN_142a78270(param_2);
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x12044) = uVar8;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef8) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11ef9) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x1cf20) = uVar8;
    FUN_140cfb6d0(param_2,param_1 + 0x1bee8,param_4);
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11c0b) = uVar3;
    uVar3 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x11bde) = uVar3;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11be8) = uVar8;
    FUN_140cfbaf0(param_2,param_1 + 0x11e78,param_4);
code_r0x000140d0c3fb:
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11c14) = uVar8;
    uVar8 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x11c18) = uVar8;
  }
  FUN_140ce8b00(*(undefined8 *)(param_1 + 0x11318),0,uVar6);
  FUN_140ce8b00(*(undefined8 *)(param_1 + 0x11318),1,uVar7);
  if (param_3 < 0xa2) {
    aiStack_188[0] = 0;
    puVar1 = (undefined1 *)(param_1 + 0x11c38);
    FUN_140cfb780(param_2,puVar1,aiStack_188,param_4);
    cVar4 = FUN_140994e70(param_1 + -0x10);
    if (((cVar4 != '\0') && (cVar4 = FUN_140731ba0(puVar1), cVar4 == '\0')) && (aiStack_188[0] == 1)
       ) {
      FUN_140702840(auStack_160,puVar1);
      uVar9 = FUN_1409a0e90(param_1 + -0x10);
      cVar4 = FUN_14098d0f0(auStack_160,uVar9,0);
      if ((cVar4 != '\0') && (auStack_160 != puVar1)) {
        FUN_140714550(puVar1,auStack_160);
      }
      FUN_140709240(auStack_160);
    }
  }
  FUN_1443c9590(uStack_40 ^ (ulonglong)auStack_1a8);
  return;
}


/* VA 140cf0ad0 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_140cf0ad0(longlong param_1,longlong param_2,undefined8 param_3)

{
  int iVar1;
  code *pcVar2;
  bool bVar3;
  char cVar4;
  char cVar5;
  ushort uVar6;
  ushort uVar7;
  uint uVar8;
  longlong lVar9;
  ulonglong uVar10;
  longlong lVar11;
  uint uVar12;
  undefined1 auStack_e8 [32];
  undefined1 auStack_c8 [32];
  longlong lStack_a8;
  longlong lStack_a0;
  undefined8 uStack_98;
  undefined8 uStack_90;
  ulonglong uStack_88;
  longlong lStack_78;
  undefined8 uStack_70;
  longlong lStack_68;
  undefined4 uStack_60;
  undefined1 uStack_5c;
  undefined2 uStack_5a;
  undefined4 uStack_58;
  undefined4 uStack_54;
  undefined2 uStack_50;
  ulonglong uStack_48;

  uStack_48 = _DAT_14a2a58c0 ^ (ulonglong)auStack_e8;
  uVar8 = FUN_142a78270();
  uVar12 = 0;
  if (uVar8 != 0) {
    do {
      lStack_a8 = 0x144e75dc8;
      uStack_98 = 0;
      uStack_90 = 0;
      uStack_88 = 0xf;
      lStack_a0 = 0;
      lStack_78 = 0;
      uStack_70 = 0;
      lStack_68 = 0;
      uStack_50 = 0;
      uStack_5c = 1;
      uStack_5a = 0;
      uStack_60 = 0x3f000000;
      uStack_58 = 0;
      uStack_54 = 0x3ea8f5c3;
      if (pcRam0000000144e75df8 != _guard_check_icall) {
        (*pcRam0000000144e75df8)(&lStack_a8);
      }
      cVar4 = FUN_142a780c0(param_1);
      if (*(code **)(lStack_a8 + 0x18) == FUN_14050f330) {
        cVar5 = '\0';
      }
      else {
        cVar5 = (**(code **)(lStack_a8 + 0x18))(&lStack_a8);
      }
      if (cVar4 != cVar5) {
        FUN_1407fac40(auStack_c8);
        _CxxThrowException(auStack_c8,0x149e36ef0);
        pcVar2 = (code *)swi(3);
        (*pcVar2)();
        return;
      }
      uVar6 = FUN_142a78220(param_1);
      if (*(code **)(lStack_a8 + 0x10) == FUN_1405837a0) {
        uVar7 = 0x50;
      }
      else {
        uVar7 = (**(code **)(lStack_a8 + 0x10))(&lStack_a8);
      }
      if (uVar7 < uVar6) goto code_r0x000140cf0d63;
      if (*(code **)(lStack_a8 + 0x40) == FUN_140cfedf0) {
        FUN_140cfedf0();
      }
      else {
        (**(code **)(lStack_a8 + 0x40))(&lStack_a8,param_1,uVar6,param_3);
      }
      if (*(longlong *)(param_1 + 8) == 0) {
code_r0x000140cf0d97:
        FUN_1407fa9d0(auStack_c8);
        _CxxThrowException(auStack_c8,0x149e36ff8);
        pcVar2 = (code *)swi(3);
        (*pcVar2)();
        return;
      }
      iVar1 = *(int *)(*(longlong *)(param_1 + 8) + 0xc);
      if ((iVar1 == 3) || (iVar1 == 0)) {
        bVar3 = true;
      }
      else {
        bVar3 = false;
      }
      if (bVar3) goto code_r0x000140cf0d97;
      if (*(code **)(lStack_a8 + 0x38) != _guard_check_icall) {
        (**(code **)(lStack_a8 + 0x38))(&lStack_a8,uVar6,param_3);
      }
      lVar11 = *(longlong *)(param_2 + 8);
      if (lVar11 == *(longlong *)(param_2 + 0x10)) {
        FUN_140cf2b80(param_2,lVar11,&lStack_a8);
      }
      else {
        FUN_140cf37b0(lVar11,&lStack_a8);
        *(longlong *)(param_2 + 8) = *(longlong *)(param_2 + 8) + 0x60;
      }
      if (lStack_78 != 0) {
        lVar9 = lStack_68 - lStack_78;
        uVar10 = (lVar9 / 0xc) * 0xc;
        lVar11 = lStack_78;
        if (uVar10 < 0x1000) {
code_r0x000140cf0cdb:
          free(lVar11,uVar10);
          lStack_78 = 0;
          uStack_70 = 0;
          lStack_68 = 0;
          goto code_r0x000140cf0cef;
        }
        uVar10 = uVar10 + 0x27;
        lVar11 = *(longlong *)(lStack_78 + -8);
        if ((lStack_78 - lVar11) - 8U < 0x20) goto code_r0x000140cf0cdb;
code_r0x000140cf0d5c:
        _invalid_parameter_noinfo_noreturn(lVar9,uVar10);
code_r0x000140cf0d63:
        FUN_1407f6f60(auStack_c8);
        _CxxThrowException(auStack_c8,0x149e36f88);
        pcVar2 = (code *)swi(3);
        (*pcVar2)();
        return;
      }
code_r0x000140cf0cef:
      if (0xf < uStack_88) {
        if (0xfff < uStack_88 + 1) {
          uVar10 = uStack_88 + 0x28;
          lVar9 = *(longlong *)(lStack_a0 + -8);
          if (0x1f < (lStack_a0 - lVar9) - 8U) goto code_r0x000140cf0d5c;
        }
        free();
      }
      uVar12 = uVar12 + 1;
    } while (uVar12 < uVar8);
  }
  FUN_1443c9590(uStack_48 ^ (ulonglong)auStack_e8);
  return;
}


/* VA 140cf0dc0 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_140cf0dc0(longlong param_1,longlong param_2,undefined8 param_3)

{
  int iVar1;
  longlong lVar2;
  code *pcVar3;
  bool bVar4;
  undefined8 uVar5;
  undefined8 uVar6;
  char cVar7;
  char cVar8;
  ushort uVar9;
  ushort uVar10;
  uint uVar11;
  uint uVar12;
  undefined1 auStack_1d8 [32];
  undefined1 auStack_1b8 [32];
  longlong lStack_198;
  undefined4 uStack_190;
  undefined4 uStack_18c;
  longlong lStack_188;
  undefined8 uStack_180;
  undefined8 uStack_178;
  ulonglong uStack_170;
  longlong lStack_168;
  undefined8 uStack_160;
  undefined8 uStack_158;
  undefined8 uStack_150;
  undefined8 uStack_148;
  undefined8 uStack_140;
  undefined8 uStack_138;
  undefined8 uStack_130;
  undefined8 uStack_128;
  undefined8 uStack_120;
  undefined8 uStack_118;
  undefined8 uStack_110;
  undefined8 uStack_108;
  undefined8 uStack_100;
  undefined8 uStack_f8;
  undefined8 uStack_f0;
  undefined8 uStack_e8;
  undefined8 uStack_e0;
  undefined8 uStack_d8;
  undefined8 uStack_d0;
  undefined8 uStack_c8;
  undefined8 uStack_c0;
  undefined8 uStack_b8;
  undefined8 uStack_b0;
  undefined8 uStack_a8;
  undefined8 uStack_a0;
  undefined8 uStack_98;
  undefined8 uStack_90;
  undefined8 uStack_88;
  undefined8 uStack_80;
  undefined8 uStack_78;
  undefined8 uStack_70;
  undefined8 uStack_68;
  ulonglong uStack_58;

  uStack_58 = _DAT_14a2a58c0 ^ (ulonglong)auStack_1d8;
  uVar11 = FUN_142a78270();
  uVar6 = uRam000000014471cba8;
  uVar5 = _DAT_14471cba0;
  uVar12 = 0;
  if (uVar11 != 0) {
    do {
      lStack_198 = 0x144714ec8;
      uStack_180 = 0;
      uStack_178 = 0;
      uStack_170 = 0xf;
      lStack_188 = 0;
      lStack_168 = 0;
      uStack_160 = 0xffffffffffffffff;
      uStack_158 = uVar5;
      uStack_150 = uVar6;
      uStack_148 = uVar5;
      uStack_140 = uVar6;
      uStack_138 = uVar5;
      uStack_130 = uVar6;
      uStack_128 = uVar5;
      uStack_120 = uVar6;
      uStack_118 = uVar5;
      uStack_110 = uVar6;
      uStack_108 = uVar5;
      uStack_100 = uVar6;
      uStack_f8 = uVar5;
      uStack_f0 = uVar6;
      uStack_e8 = uVar5;
      uStack_e0 = uVar6;
      uStack_d8 = uVar5;
      uStack_d0 = uVar6;
      uStack_c8 = uVar5;
      uStack_c0 = uVar6;
      uStack_b8 = uVar5;
      uStack_b0 = uVar6;
      uStack_a8 = uVar5;
      uStack_a0 = uVar6;
      uStack_98 = uVar5;
      uStack_90 = uVar6;
      uStack_88 = uVar5;
      uStack_80 = uVar6;
      uStack_78 = uVar5;
      uStack_70 = uVar6;
      uStack_68 = 0xffffffffffffffff;
      uStack_190 = 0;
      uStack_18c = 0xffffffff;
      if (pcRam0000000144714ef8 != _guard_check_icall) {
        (*pcRam0000000144714ef8)(&lStack_198);
      }
      cVar7 = FUN_142a780c0(param_1);
      if (*(code **)(lStack_198 + 0x18) == FUN_14050f330) {
        cVar8 = '\0';
      }
      else {
        cVar8 = (**(code **)(lStack_198 + 0x18))(&lStack_198);
      }
      if (cVar7 != cVar8) {
        FUN_1407fac40(auStack_1b8);
        _CxxThrowException(auStack_1b8,0x149e36ef0);
        pcVar3 = (code *)swi(3);
        (*pcVar3)();
        return;
      }
      uVar9 = FUN_142a78220(param_1);
      if (*(code **)(lStack_198 + 0x10) == FUN_1405837a0) {
        uVar10 = 0x50;
      }
      else {
        uVar10 = (**(code **)(lStack_198 + 0x10))(&lStack_198);
      }
      if (uVar10 < uVar9) {
        FUN_1407f6f60(auStack_1b8);
        _CxxThrowException(auStack_1b8,0x149e36f88);
        pcVar3 = (code *)swi(3);
        (*pcVar3)();
        return;
      }
      if (*(code **)(lStack_198 + 0x40) == FUN_140cfeea0) {
        FUN_140cfeea0();
      }
      else {
        (**(code **)(lStack_198 + 0x40))(&lStack_198,param_1,uVar9,param_3);
      }
      if (*(longlong *)(param_1 + 8) == 0) {
code_r0x000140cf10a1:
        FUN_1407fa9d0(auStack_1b8);
        _CxxThrowException(auStack_1b8,0x149e36ff8);
        pcVar3 = (code *)swi(3);
        (*pcVar3)();
        return;
      }
      iVar1 = *(int *)(*(longlong *)(param_1 + 8) + 0xc);
      if ((iVar1 == 3) || (iVar1 == 0)) {
        bVar4 = true;
      }
      else {
        bVar4 = false;
      }
      if (bVar4) goto code_r0x000140cf10a1;
      if (*(code **)(lStack_198 + 0x38) != _guard_check_icall) {
        (**(code **)(lStack_198 + 0x38))(&lStack_198,uVar9,param_3);
      }
      lVar2 = *(longlong *)(param_2 + 8);
      if (lVar2 == *(longlong *)(param_2 + 0x10)) {
        FUN_1407d0760(param_2,lVar2,&lStack_198);
      }
      else {
        FUN_1407023f0(lVar2,&lStack_198);
        *(longlong *)(param_2 + 8) = *(longlong *)(param_2 + 8) + 0x140;
      }
      lVar2 = lStack_168;
      lStack_198 = 0x144714ec8;
      if (lStack_168 != 0) {
        FUN_140a76ff0(lStack_168);
        free(lVar2,0x20808);
      }
      if (0xf < uStack_170) {
        if (0xfff < uStack_170 + 1) {
          if (0x1f < (lStack_188 - *(longlong *)(lStack_188 + -8)) - 8U) {
            _invalid_parameter_noinfo_noreturn(*(longlong *)(lStack_188 + -8),uStack_170 + 0x28);
            pcVar3 = (code *)swi(3);
            (*pcVar3)();
            return;
          }
        }
        free();
      }
      uVar12 = uVar12 + 1;
    } while (uVar12 < uVar11);
  }
  FUN_1443c9590(uStack_58 ^ (ulonglong)auStack_1d8);
  return;
}


/* VA 140cfedf0 */

void FUN_140cfedf0(longlong param_1,undefined8 param_2,short param_3,undefined8 param_4)

{
  code *pcVar1;
  undefined1 uVar2;
  undefined2 uVar3;
  undefined4 uVar4;
  undefined1 auStack_28 [32];

  if (param_3 == 0x50) {
    uVar4 = FUN_142a78270(param_2);
    *(undefined4 *)(param_1 + 0x28) = uVar4;
    FUN_140cfbaf0(param_2,param_1 + 8,param_4);
    uVar4 = FUN_142a780f0(param_2);
    *(undefined4 *)(param_1 + 0x48) = uVar4;
    uVar4 = FUN_142a780f0(param_2);
    *(undefined4 *)(param_1 + 0x50) = uVar4;
    uVar4 = FUN_142a780f0(param_2);
    *(undefined4 *)(param_1 + 0x54) = uVar4;
    uVar2 = FUN_142a780c0(param_2);
    *(undefined1 *)(param_1 + 0x4c) = uVar2;
    uVar3 = FUN_142a78220(param_2);
    *(undefined2 *)(param_1 + 0x4e) = uVar3;
    return;
  }
  FUN_1407fab50(auStack_28);
  _CxxThrowException(auStack_28,0x149e370d8);
  pcVar1 = (code *)swi(3);
  (*pcVar1)();
  return;
}


/* VA 140cfeea0 */

ulonglong FUN_140cfeea0(longlong param_1,undefined8 param_2,short param_3,undefined8 param_4)

{
  code *pcVar1;
  uint uVar2;
  int iVar3;
  undefined4 uVar4;
  ulonglong uVar5;
  undefined4 *puVar6;
  uint uVar7;
  undefined1 auStack_28 [32];

  if (param_3 != 0x50) {
    FUN_1407fab50(auStack_28);
    _CxxThrowException(auStack_28,0x149e370d8);
    pcVar1 = (code *)swi(3);
    uVar5 = (*pcVar1)();
    return uVar5;
  }
  iVar3 = FUN_142a78270(param_2);
  if (iVar3 == 1) {
    uVar4 = 1;
  }
  else {
    if (iVar3 != 2) {
      FUN_140a874c0(auStack_28);
      _CxxThrowException(auStack_28,0x149e37590);
      pcVar1 = (code *)swi(3);
      uVar5 = (*pcVar1)();
      return uVar5;
    }
    uVar4 = 2;
  }
  *(undefined4 *)(param_1 + 8) = uVar4;
  uVar4 = FUN_142a78270(param_2);
  *(undefined4 *)(param_1 + 0xc) = uVar4;
  FUN_140cfbaf0(param_2,param_1 + 0x10,param_4);
  puVar6 = (undefined4 *)(param_1 + 0x38);
  uVar2 = FUN_142a78270(param_2);
  uVar7 = 0;
  if (uVar2 != 0) {
    do {
      if (puVar6 == (undefined4 *)(param_1 + 0x138)) break;
      uVar4 = FUN_142a78270(param_2);
      *puVar6 = uVar4;
      uVar7 = uVar7 + 1;
      puVar6 = puVar6 + 1;
    } while (uVar7 < uVar2);
  }
  return (ulonglong)uVar7;
}


/* VA 140cfbaf0 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_140cfbaf0(undefined8 param_1,undefined8 param_2)

{
  code *pcVar1;
  undefined8 uVar2;
  longlong lVar3;
  ulonglong uVar4;
  undefined1 auStack_58 [32];
  longlong alStack_38 [3];
  ulonglong uStack_20;
  ulonglong uStack_18;

  uStack_18 = _DAT_14a2a58c0 ^ (ulonglong)auStack_58;
  uVar2 = FUN_142a784b0(param_1,alStack_38);
  FUN_14052ff30(param_2,uVar2);
  if (0xf < uStack_20) {
    uVar4 = uStack_20 + 1;
    lVar3 = alStack_38[0];
    if (0xfff < uVar4) {
      lVar3 = *(longlong *)(alStack_38[0] + -8);
      uVar4 = uStack_20 + 0x28;
      if (0x1f < (alStack_38[0] - lVar3) - 8U) {
        _invalid_parameter_noinfo_noreturn();
        pcVar1 = (code *)swi(3);
        (*pcVar1)();
        return;
      }
    }
    free(lVar3,uVar4);
  }
  FUN_1443c9590(uStack_18 ^ (ulonglong)auStack_58);
  return;
}


/* VA 142a784b0 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_142a784b0(longlong param_1,undefined8 *param_2)

{
  code *pcVar1;
  uint uVar2;
  ulonglong uVar3;
  ushort *puVar4;
  ushort *puVar5;
  ushort *puVar6;
  ulonglong uVar7;
  longlong lVar8;
  undefined1 auStack_8a8 [32];
  undefined1 *puStack_888;
  undefined1 auStack_878 [8];
  undefined8 *puStack_870;
  ushort *puStack_868;
  ushort *puStack_860;
  ushort *puStack_858;
  ushort auStack_848 [1024];
  ulonglong uStack_48;

  uStack_48 = _DAT_14a2a58c0 ^ (ulonglong)auStack_8a8;
  puVar5 = (ushort *)0x0;
  puStack_870 = param_2;
  (**(code **)(**(longlong **)(param_1 + 8) + 0x10))(*(longlong **)(param_1 + 8),&puStack_870,4);
  uVar2 = (uint)puStack_870;
  if (*(int *)(param_1 + 0x10) != 1) {
    uVar2 = (((uint)puStack_870 & 0xffff) >> 8 | (uint)(ushort)((short)puStack_870 << 8)) << 0x10 |
            (uint)puStack_870 >> 0x18 | (uint)(ushort)((short)((ulonglong)puStack_870 >> 0x10) << 8)
    ;
  }
  uVar7 = (ulonglong)uVar2;
  if (uVar2 != 0) {
    uVar3 = uVar7 * 2;
    if (0x3ff < uVar2) {
      puStack_868 = (ushort *)0x0;
      puStack_860 = (ushort *)0x0;
      puStack_858 = (ushort *)0x0;
      if (uVar2 == 0) {
        puVar5 = (ushort *)0x0;
      }
      else {
        if (uVar7 != 0) {
          if (uVar3 < 0x1000) {
            puVar5 = (ushort *)FUN_1443c9194(uVar3);
          }
          else {
            if (uVar3 + 0x27 <= uVar3) {
              FUN_140517340();
              pcVar1 = (code *)swi(3);
              (*pcVar1)();
              return;
            }
            lVar8 = FUN_1443c9194();
            if (lVar8 == 0) goto LAB_142a786ff;
            puVar5 = (ushort *)(lVar8 + 0x27U & 0xffffffffffffffe0);
            *(longlong *)(puVar5 + -4) = lVar8;
          }
        }
        puStack_868 = puVar5;
        puStack_858 = puVar5 + uVar7;
        memset(puVar5,0,uVar3);
        puStack_860 = puVar5 + uVar7;
      }
      puVar4 = puStack_860;
      (**(code **)(**(longlong **)(param_1 + 8) + 0x10))(*(longlong **)(param_1 + 8),puVar5,uVar3);
      puVar6 = puVar5;
      if (*(int *)(param_1 + 0x10) != 1) {
        for (; uVar7 != 0; uVar7 = uVar7 - 1) {
          *puVar6 = *puVar6 >> 8 | *puVar6 << 8;
          puVar6 = puVar6 + 1;
        }
      }
      lVar8 = (longlong)puVar4 - (longlong)puVar5 >> 1;
      if (lVar8 == 0) {
        *param_2 = 0;
        param_2[1] = 0;
        param_2[2] = 0;
        param_2[3] = 0xf;
        *(undefined1 *)param_2 = 0;
      }
      else {
        puStack_888 = auStack_878;
        FUN_1406fb430(param_2,puVar5,puVar5 + lVar8,0);
      }
      if (puVar5 != (ushort *)0x0) {
        uVar7 = ((longlong)puVar4 - (longlong)puVar5 >> 1) * 2;
        puVar6 = puVar5;
        if (0xfff < uVar7) {
          uVar7 = uVar7 + 0x27;
          puVar6 = *(ushort **)(puVar5 + -4);
          if (0x1f < (ulonglong)((longlong)puVar5 + (-8 - (longlong)puVar6))) {
LAB_142a786ff:
            _invalid_parameter_noinfo_noreturn();
            pcVar1 = (code *)swi(3);
            (*pcVar1)();
            return;
          }
        }
        free(puVar6,uVar7);
      }
      goto LAB_142a78711;
    }
    (**(code **)(**(longlong **)(param_1 + 8) + 0x10))
              (*(longlong **)(param_1 + 8),auStack_848,uVar3);
    puVar5 = auStack_848;
    if (*(int *)(param_1 + 0x10) != 1) {
      for (; uVar7 != 0; uVar7 = uVar7 - 1) {
        *puVar5 = *puVar5 >> 8 | *puVar5 << 8;
        puVar5 = puVar5 + 1;
      }
    }
    if ((longlong)uVar3 >> 1 != 0) {
      puStack_888 = auStack_878;
      FUN_1406fb430(param_2,auStack_848,auStack_848 + ((longlong)uVar3 >> 1),0);
      goto LAB_142a78711;
    }
  }
  *param_2 = 0;
  param_2[1] = 0;
  param_2[2] = 0;
  param_2[3] = 0xf;
  *(undefined1 *)param_2 = 0;
LAB_142a78711:
  FUN_1443c9590(uStack_48 ^ (ulonglong)auStack_8a8);
  return;
}


/* VA 140cf14f0 */

void FUN_140cf14f0(undefined8 param_1,undefined8 param_2,undefined8 param_3)

{
  int iVar1;

  iVar1 = FUN_142a78270();
  if (iVar1 != 0) {
    FUN_140cf08b0(param_1,param_2,param_3);
  }
  return;
}


/* VA 140cf08b0 */

ulonglong FUN_140cf08b0(longlong param_1,longlong param_2,undefined8 param_3)

{
  int iVar1;
  undefined8 *puVar2;
  code *pcVar3;
  bool bVar4;
  char cVar5;
  char cVar6;
  ushort uVar7;
  ushort uVar8;
  uint uVar9;
  ulonglong uVar10;
  uint uVar11;
  undefined1 auStack_a8 [32];
  longlong lStack_88;
  undefined1 uStack_80;
  undefined1 uStack_7f;
  undefined4 uStack_7c;
  undefined8 uStack_78;
  undefined4 uStack_60;
  undefined4 uStack_5c;
  undefined1 uStack_58;
  undefined1 uStack_57;
  undefined2 uStack_56;
  undefined4 uStack_54;
  undefined4 uStack_50;
  undefined4 uStack_4c;
  undefined4 uStack_48;
  undefined4 uStack_44;
  undefined8 uStack_40;
  undefined4 uStack_38;
  undefined4 uStack_34;
  undefined4 uStack_30;
  undefined1 uStack_2c;
  undefined2 uStack_2a;

  uVar9 = FUN_142a78270();
  uVar11 = 0;
  if (uVar9 != 0) {
    do {
      FUN_1407f28a0(&lStack_88);
      (**(code **)(lStack_88 + 0x30))(&lStack_88);
      cVar5 = FUN_142a780c0(param_1);
      cVar6 = (**(code **)(lStack_88 + 0x18))(&lStack_88);
      if (cVar5 != cVar6) {
        FUN_1407fac40(auStack_a8);
        _CxxThrowException(auStack_a8,0x149e36ef0);
        pcVar3 = (code *)swi(3);
        uVar10 = (*pcVar3)();
        return uVar10;
      }
      uVar7 = FUN_142a78220(param_1);
      uVar8 = (**(code **)(lStack_88 + 0x10))(&lStack_88);
      if (uVar8 < uVar7) {
        FUN_1407f6f60(auStack_a8);
        _CxxThrowException(auStack_a8,0x149e36f88);
        pcVar3 = (code *)swi(3);
        uVar10 = (*pcVar3)();
        return uVar10;
      }
      (**(code **)(lStack_88 + 0x40))(&lStack_88,param_1,uVar7,param_3);
      if (*(longlong *)(param_1 + 8) == 0) {
code_r0x000140cf0ab5:
        FUN_1407fa9d0(auStack_a8);
        _CxxThrowException(auStack_a8,0x149e36ff8);
        pcVar3 = (code *)swi(3);
        uVar10 = (*pcVar3)();
        return uVar10;
      }
      iVar1 = *(int *)(*(longlong *)(param_1 + 8) + 0xc);
      if ((iVar1 == 3) || (iVar1 == 0)) {
        bVar4 = true;
      }
      else {
        bVar4 = false;
      }
      if (bVar4) goto code_r0x000140cf0ab5;
      (**(code **)(lStack_88 + 0x38))(&lStack_88,uVar7,param_3);
      puVar2 = *(undefined8 **)(param_2 + 8);
      if (puVar2 == *(undefined8 **)(param_2 + 0x10)) {
        FUN_140696a40(param_2,puVar2,&lStack_88);
      }
      else {
        *puVar2 = 0x144757d28;
        *(undefined1 *)(puVar2 + 1) = uStack_80;
        *(undefined1 *)((longlong)puVar2 + 9) = uStack_7f;
        *(undefined4 *)((longlong)puVar2 + 0xc) = uStack_7c;
        puVar2[2] = uStack_78;
        *(undefined4 *)(puVar2 + 5) = uStack_60;
        *(undefined4 *)((longlong)puVar2 + 0x2c) = uStack_5c;
        *(undefined1 *)(puVar2 + 6) = uStack_58;
        *(undefined1 *)((longlong)puVar2 + 0x31) = uStack_57;
        *(undefined2 *)((longlong)puVar2 + 0x32) = uStack_56;
        *(undefined4 *)((longlong)puVar2 + 0x34) = uStack_54;
        *(undefined4 *)(puVar2 + 7) = uStack_50;
        *(undefined4 *)((longlong)puVar2 + 0x3c) = uStack_4c;
        *(undefined4 *)(puVar2 + 8) = uStack_48;
        *(undefined4 *)((longlong)puVar2 + 0x44) = uStack_44;
        puVar2[9] = uStack_40;
        *(undefined4 *)(puVar2 + 10) = uStack_38;
        *(undefined4 *)((longlong)puVar2 + 0x54) = uStack_34;
        *(undefined4 *)(puVar2 + 0xb) = uStack_30;
        *(undefined1 *)((longlong)puVar2 + 0x5c) = uStack_2c;
        *(undefined2 *)((longlong)puVar2 + 0x5e) = uStack_2a;
        *(longlong *)(param_2 + 8) = *(longlong *)(param_2 + 8) + 0x60;
      }
      uVar11 = uVar11 + 1;
    } while (uVar11 < uVar9);
  }
  return (ulonglong)uVar11;
}


/* VA 140cefcf0 */

void FUN_140cefcf0(longlong param_1,longlong param_2,undefined8 param_3)

{
  code *pcVar1;
  bool bVar2;
  char cVar3;
  char cVar4;
  ushort uVar5;
  ushort uVar6;
  int iVar7;
  uint uVar8;
  int iVar9;
  longlong *plVar10;
  uint uVar11;
  longlong *plVar12;
  undefined1 auStack_38 [32];

  iVar7 = FUN_142a78270();
  FUN_14073c3a0(param_2,iVar7);
  if (iVar7 != 0) {
    plVar10 = *(longlong **)(param_2 + 8);
    iVar9 = 0x60;
    if (0 < *(int *)(param_2 + 0x34)) {
      iVar9 = *(int *)(param_2 + 0x34);
    }
    plVar12 = (longlong *)((longlong)(iVar9 * iVar7) + (longlong)plVar10);
    uVar8 = FUN_142a78270(param_1);
    uVar11 = 0;
    if (uVar8 != 0) {
      do {
        if (plVar10 == plVar12) {
          return;
        }
        (**(code **)(*plVar10 + 0x30))(plVar10);
        cVar3 = FUN_142a780c0(param_1);
        cVar4 = (**(code **)(*plVar10 + 0x18))(plVar10);
        if (cVar3 != cVar4) {
          FUN_1407fac40(auStack_38);
          _CxxThrowException(auStack_38,0x149e36ef0);
          pcVar1 = (code *)swi(3);
          (*pcVar1)();
          return;
        }
        uVar5 = FUN_142a78220(param_1);
        uVar6 = (**(code **)(*plVar10 + 0x10))(plVar10);
        if (uVar6 < uVar5) {
          FUN_1407f6f60(auStack_38);
          _CxxThrowException(auStack_38,0x149e36f88);
          pcVar1 = (code *)swi(3);
          (*pcVar1)();
          return;
        }
        (**(code **)(*plVar10 + 0x40))(plVar10,param_1,uVar5,param_3);
        if (*(longlong *)(param_1 + 8) == 0) {
LAB_140cefe36:
          FUN_1407fa9d0(auStack_38);
          _CxxThrowException(auStack_38,0x149e36ff8);
          pcVar1 = (code *)swi(3);
          (*pcVar1)();
          return;
        }
        iVar7 = *(int *)(*(longlong *)(param_1 + 8) + 0xc);
        if ((iVar7 == 3) || (iVar7 == 0)) {
          bVar2 = true;
        }
        else {
          bVar2 = false;
        }
        if (bVar2) goto LAB_140cefe36;
        (**(code **)(*plVar10 + 0x38))(plVar10,uVar5,param_3);
        uVar11 = uVar11 + 1;
        plVar10 = plVar10 + 0xc;
      } while (uVar11 < uVar8);
    }
  }
  return;
}


/* VA 140cf25a0 */

void FUN_140cf25a0(longlong param_1,longlong *param_2,undefined8 param_3)

{
  int iVar1;
  longlong *plVar2;
  code *pcVar3;
  bool bVar4;
  undefined1 uVar5;
  undefined2 uVar6;
  undefined4 uVar7;
  undefined4 uVar8;
  longlong lVar9;
  ulonglong uVar10;
  longlong *plVar11;
  int iVar12;
  undefined1 auStack_38 [32];

  lVar9 = (param_2[1] - *param_2) / 6 + (param_2[1] - *param_2 >> 0x3f);
  uVar10 = (lVar9 >> 4) - (lVar9 >> 0x3f);
  FUN_142a7ddb0(param_1,uVar10 & 0xffffffff);
  if ((int)uVar10 != 0) {
    plVar11 = (longlong *)*param_2;
    plVar2 = (longlong *)param_2[1];
    uVar8 = 0xffffffff;
    if (*(longlong **)(param_1 + 8) == (longlong *)0x0) {
      uVar7 = 0xffffffff;
    }
    else {
      uVar7 = (**(code **)(**(longlong **)(param_1 + 8) + 0x20))();
    }
    FUN_142a7ddb0(param_1,0);
    iVar12 = 0;
    if (plVar11 != plVar2) {
      do {
        (**(code **)(*plVar11 + 0x20))(plVar11,param_3);
        uVar5 = (**(code **)(*plVar11 + 0x18))(plVar11);
        FUN_142a7dba0(param_1,uVar5);
        uVar6 = (**(code **)(*plVar11 + 0x10))(plVar11);
        FUN_142a7dd70(param_1,uVar6);
        (**(code **)(*plVar11 + 0x48))(plVar11,param_1,param_3);
        if (*(longlong *)(param_1 + 8) == 0) {
FUN_140cf271e:
          FUN_1407faa90(auStack_38);
          _CxxThrowException(auStack_38,0x149e37068);
          pcVar3 = (code *)swi(3);
          (*pcVar3)();
          return;
        }
        iVar1 = *(int *)(*(longlong *)(param_1 + 8) + 0xc);
        if ((iVar1 == 3) || (iVar1 == 0)) {
          bVar4 = true;
        }
        else {
          bVar4 = false;
        }
        if (bVar4) goto FUN_140cf271e;
        (**(code **)(*plVar11 + 0x28))(plVar11,param_3);
        iVar12 = iVar12 + 1;
        plVar11 = plVar11 + 0xc;
      } while (plVar11 != plVar2);
      if (iVar12 != 0) {
        if (*(longlong **)(param_1 + 8) != (longlong *)0x0) {
          uVar8 = (**(code **)(**(longlong **)(param_1 + 8) + 0x20))();
        }
        plVar11 = *(longlong **)(param_1 + 8);
        if (plVar11 != (longlong *)0x0) {
          (**(code **)(*plVar11 + 0x30))(plVar11,uVar7,0);
        }
        FUN_142a7ddb0(param_1,iVar12);
        plVar11 = *(longlong **)(param_1 + 8);
        if (plVar11 != (longlong *)0x0) {
          (**(code **)(*plVar11 + 0x30))(plVar11,uVar8,0);
        }
      }
    }
  }
  return;
}


/* VA 140993fc0 */

uint FUN_140993fc0(int param_1,int param_2)

{
  if (0x1382 < param_1 - 0x17b3U) {
    return 0xffffffff;
  }
  if (param_2 == 0) {
    return param_1 - 0x17b3U;
  }
  if (param_2 == 1) {
    return param_1 - 0x1b9a;
  }
  if (param_2 != 2) {
    if (param_2 == 3) {
      return param_1 - 0x2368;
    }
    if (param_2 != 4) {
      return 0;
    }
    return param_1 - 0x274f;
  }
  return param_1 - 0x1f81;
}


/* VA 1407a3ba0 */

longlong FUN_1407a3ba0(longlong param_1,undefined8 param_2,int param_3)

{
  char cVar1;
  longlong lVar2;
  longlong lVar3;

  lVar3 = (longlong)param_3;
  cVar1 = FUN_1407a45e0(param_1,param_2,param_3);
  if (cVar1 != '\0') {
    lVar2 = 0;
    switch((int)param_2) {
    case 1:
      return lVar3 * 0x1f0 + *(longlong *)(param_1 + 0x84a8);
    case 2:
      return lVar3 * 0x208 + *(longlong *)(param_1 + 0x84f8);
    case 3:
      return lVar3 * 0x220 + *(longlong *)(param_1 + 34000);
    case 4:
      return lVar3 * 0x1e8 + *(longlong *)(param_1 + 0x8520);
    case 5:
      return lVar3 * 0x1f8 + *(longlong *)(param_1 + 0x8548);
    case 6:
      return lVar3 * 0x3f0 + *(longlong *)(param_1 + 0x8570);
    case 7:
      return lVar3 * 0x600 + *(longlong *)(param_1 + 0x8598);
    case 8:
      return lVar3 * 0x1d8 + *(longlong *)(param_1 + 0x85c0);
    case 9:
      return lVar3 * 0x218 + *(longlong *)(param_1 + 0x85e8);
    case 10:
      return lVar3 * 0x1f8 + *(longlong *)(param_1 + 0x8638);
    case 0xb:
      return lVar3 * 0x228 + *(longlong *)(param_1 + 0x8660);
    case 0xc:
      return lVar3 * 0x230 + *(longlong *)(param_1 + 0x8610);
    case 0xd:
      return lVar3 * 0xd18 + *(longlong *)(param_1 + 0x86b0);
    case 0xe:
      return lVar3 * 0x1e0 + *(longlong *)(param_1 + 0x8688);
    case 0xf:
      lVar2 = lVar3 * 0x91c8 + *(longlong *)(param_1 + 0x86d8);
      break;
    case 0x10:
      return lVar3 * 0x1b0 + *(longlong *)(param_1 + 0x8700);
    }
    return lVar2;
  }
  return 0;
}


/* VA 1407a5760 */

void FUN_1407a5760(longlong param_1,int param_2,int param_3,int param_4,uint param_5)

{
  int iVar1;
  int iVar2;
  char cVar3;
  int iVar4;
  longlong lVar5;
  uint uVar6;
  longlong lVar7;
  undefined4 uVar8;
  longlong lVar9;
  bool bVar10;

  lVar7 = (longlong)param_3;
  cVar3 = FUN_1407a45e0();
  if (cVar3 == '\0') {
    return;
  }
  bVar10 = false;
  uVar8 = 0xffffffff;
  switch(param_2) {
  case 1:
    lVar7 = lVar7 * 0x1f0;
    bVar10 = *(uint *)(*(longlong *)(param_1 + 0x84a8) + 0x70 + lVar7) != (uint)(param_4 != 0);
    if (bVar10) {
      *(uint *)(*(longlong *)(param_1 + 0x84a8) + 0x70 + lVar7) = (uint)(param_4 != 0);
    }
    uVar8 = *(undefined4 *)(lVar7 + 8 + *(longlong *)(param_1 + 0x84a8));
    break;
  case 2:
    lVar9 = *(longlong *)(param_1 + 0x84f8) + lVar7 * 0x208;
    iVar1 = *(int *)(lVar9 + 0x1b0);
    iVar2 = *(int *)(lVar9 + 0x1b4);
    iVar4 = iVar2;
    if (iVar1 <= iVar2) {
      iVar4 = iVar1;
      iVar1 = iVar2;
    }
    if (param_4 < iVar4) {
      param_4 = iVar4;
    }
    if (iVar1 < param_4) {
      param_4 = iVar1;
    }
    bVar10 = *(int *)(lVar9 + 0x70) != param_4;
    if (bVar10) {
      *(int *)(lVar9 + 0x70) = param_4;
    }
    uVar8 = *(undefined4 *)(lVar7 * 0x208 + 8 + *(longlong *)(param_1 + 0x84f8));
    break;
  case 3:
    *(int *)(lVar7 * 0x220 + 0x70 + *(longlong *)(param_1 + 34000)) = param_4;
    return;
  case 4:
    lVar9 = *(longlong *)(param_1 + 0x8520) + lVar7 * 0x1e8;
    iVar1 = *(int *)(lVar9 + 0x1b0);
    iVar2 = *(int *)(lVar9 + 0x1b4);
    iVar4 = iVar2;
    if (iVar1 <= iVar2) {
      iVar4 = iVar1;
      iVar1 = iVar2;
    }
    if (param_4 < iVar4) {
      param_4 = iVar4;
    }
    if (iVar1 < param_4) {
      param_4 = iVar1;
    }
    bVar10 = *(int *)(lVar9 + 0x70) != param_4;
    if (bVar10) {
      *(int *)(lVar9 + 0x70) = param_4;
    }
    uVar8 = *(undefined4 *)(lVar7 * 0x1e8 + 8 + *(longlong *)(param_1 + 0x8520));
    break;
  case 6:
    if (-1 < (int)param_5) {
      lVar9 = *(longlong *)(param_1 + 0x8570);
      if ((int)param_5 < *(int *)(lVar7 * 0x3f0 + 0x3b0 + lVar9)) {
        lVar5 = lVar7 * 0xfc + (longlong)(int)param_5;
        bVar10 = *(int *)(lVar9 + 0x1b0 + lVar5 * 4) != param_4;
        if (bVar10) {
          *(int *)(lVar9 + 0x1b0 + lVar5 * 4) = param_4;
        }
        uVar8 = *(undefined4 *)(*(longlong *)(param_1 + 0x8570) + 8 + lVar7 * 0x3f0);
      }
    }
    break;
  case 7:
    if (0x100 < param_5) break;
    lVar9 = *(longlong *)(param_1 + 0x8598);
    if ((int)param_5 < 0x100) {
      lVar5 = lVar7 * 0x180 + (longlong)(int)param_5;
      bVar10 = *(int *)(lVar9 + 0x1fc + lVar5 * 4) != param_4;
      if (bVar10) {
        *(int *)(lVar9 + 0x1fc + lVar5 * 4) = param_4;
      }
      lVar9 = *(longlong *)(param_1 + 0x8598);
      lVar7 = lVar7 * 0x600;
    }
    else {
      lVar7 = lVar7 * 0x600;
      bVar10 = *(int *)(lVar9 + 0x70 + lVar7) != param_4;
      if (bVar10) {
        *(int *)(lVar9 + 0x70 + lVar7) = param_4;
      }
      lVar9 = *(longlong *)(param_1 + 0x8598);
    }
    goto code_r0x0001407a59fe;
  case 9:
    lVar9 = *(longlong *)(param_1 + 0x85e8) + lVar7 * 0x218;
    iVar1 = *(int *)(lVar9 + 0x1b0);
    iVar2 = *(int *)(lVar9 + 0x1b4);
    iVar4 = iVar2;
    if (iVar1 <= iVar2) {
      iVar4 = iVar1;
      iVar1 = iVar2;
    }
    if (param_4 < iVar4) {
      param_4 = iVar4;
    }
    if (iVar1 < param_4) {
      param_4 = iVar1;
    }
    bVar10 = *(int *)(lVar9 + 0x70) != param_4;
    if (bVar10) {
      *(int *)(lVar9 + 0x70) = param_4;
    }
    uVar8 = *(undefined4 *)(lVar7 * 0x218 + 8 + *(longlong *)(param_1 + 0x85e8));
    break;
  case 0xc:
    lVar7 = lVar7 * 0x230;
    bVar10 = *(uint *)(*(longlong *)(param_1 + 0x8610) + 0x70 + lVar7) != (uint)(param_4 != 0);
    if (bVar10) {
      *(uint *)(*(longlong *)(param_1 + 0x8610) + 0x70 + lVar7) = (uint)(param_4 != 0);
    }
    lVar9 = *(longlong *)(param_1 + 0x8610);
code_r0x0001407a59fe:
    uVar8 = *(undefined4 *)(lVar9 + 8 + lVar7);
  }
  uVar6 = 0xffffffff;
  if (param_2 == 6) {
    uVar6 = param_5;
  }
  if ((bVar10) && (cVar3 = FUN_140763f90(param_1 + 0x83a0,uVar8,uVar6), cVar3 != '\0')) {
    FUN_1407a5000(param_1,uVar8,uVar6,0xffffffff);
  }
  return;
}


/* VA 1408ab280 */

undefined4 *
FUN_1408ab280(longlong param_1,undefined4 *param_2,uint param_3,int param_4,undefined4 param_5,
             undefined4 param_6,undefined4 param_7,undefined4 param_8)

{
  char cVar1;
  int iVar2;
  longlong lVar3;
  undefined8 uStack_18;
  char cStack_10;

  if ((int)param_3 < 0) {
    lVar3 = *(longlong *)(*(longlong *)(param_1 + 0x2d7f8) + 0x7e0 + (longlong)param_4 * 8);
LAB_1408ab2e0:
    if ((((lVar3 != 0) && (cVar1 = FUN_1407a45e0(lVar3,param_5,param_6), cVar1 != '\0')) &&
        (lVar3 = FUN_1407a3ba0(lVar3,param_5,param_6), lVar3 != 0)) &&
       (iVar2 = FUN_14076c540(param_7), iVar2 != 0)) {
      uStack_18._0_5_ = CONCAT14(1,param_8);
      FUN_14075a780(&uStack_18,lVar3,iVar2,uStack_18);
      if (cStack_10 != '\0') goto LAB_1408ab369;
    }
  }
  else if ((int)param_3 < 0x2000) {
    lVar3 = *(longlong *)(*(longlong *)(param_1 + 0x2d7f8) + 0x18 + (ulonglong)(param_3 >> 7) * 8);
    if (*(longlong *)(lVar3 + 0x18 + (ulonglong)(param_3 & 0x7f) * 8) != 0) {
      lVar3 = *(longlong *)
               (*(longlong *)(lVar3 + 0x18 + (ulonglong)(param_3 & 0x7f) * 8) + 0x1c078 +
               (longlong)param_4 * 8);
      goto LAB_1408ab2e0;
    }
  }
  FUN_14098fb30(&uStack_18,param_5,param_7);
LAB_1408ab369:
                    /* WARNING (jumptable): Sanity check requires truncation of jumptable */
                    /* WARNING: Could not find normalized switch variable to match jumptable */
  switch(cStack_10) {
  default:
    param_2[2] = 0;
    break;
  case '\x01':
    *(undefined1 *)(param_2 + 2) = (undefined1)uStack_18;
    *param_2 = 3;
    return param_2;
  case '\x02':
    param_2[2] = (undefined4)uStack_18;
    break;
  case '\x03':
    param_2[2] = (undefined4)uStack_18;
    *param_2 = 0;
    return param_2;
  case '\x04':
    *param_2 = 4;
    lVar3 = 0x14a601271;
    if (uStack_18 != 0) {
      lVar3 = uStack_18;
    }
    *(longlong *)(param_2 + 2) = lVar3;
    return param_2;
  }
  *param_2 = 1;
  return param_2;
}


/* VA 140898570 */

undefined8 *
FUN_140898570(longlong param_1,undefined8 *param_2,int param_3,int param_4,int param_5,uint param_6,
             int param_7,uint param_8)

{
  int iVar1;
  int iVar2;
  bool bVar3;
  int iVar4;
  undefined8 *puVar5;
  undefined8 uVar6;
  longlong lVar7;
  int *piVar8;
  longlong lVar9;
  uint uVar10;
  undefined1 uVar11;
  longlong lVar12;
  longlong lVar13;
  longlong lVar14;
  char *pcVar15;
  longlong lVar16;
  longlong lVar17;
  undefined8 in_stack_fffffffffffffe88;
  undefined4 uVar18;
  int iStack_158;
  int iStack_154;
  undefined4 uStack_150;
  undefined1 uStack_14c;
  undefined1 uStack_148;
  uint uStack_144;
  longlong alStack_140 [2];
  int iStack_130;
  undefined4 uStack_12c;
  undefined8 uStack_128;
  undefined8 uStack_120;
  char cStack_118;
  undefined8 uStack_110;
  undefined8 uStack_108;
  char cStack_100;
  undefined1 auStack_f8 [16];
  undefined1 auStack_e8 [16];
  undefined1 auStack_d8 [16];
  undefined1 auStack_c8 [16];
  undefined1 auStack_b8 [16];
  undefined1 auStack_a8 [16];
  undefined1 auStack_98 [16];
  undefined1 auStack_88 [16];
  undefined1 auStack_78 [16];
  undefined1 auStack_68 [16];
  undefined1 auStack_58 [32];

  uVar18 = (undefined4)((ulonglong)in_stack_fffffffffffffe88 >> 0x20);
  iVar4 = 0;
  if ((param_4 < 0) ||
     ((uVar11 = 0, param_4 < 0x2da &&
      (uVar11 = *(undefined1 *)((longlong)param_4 * 0x40 + 0x149e583c0),
      *(longlong *)((longlong)param_4 * 0x40 + 0x149e58398) == 0)))) {
    *(undefined4 *)param_2 = 0;
    *(undefined4 *)(param_2 + 1) = 0;
    return param_2;
  }
  uStack_150 = FUN_1408a8390(param_1);
  iStack_154 = param_5;
  iStack_158 = param_4;
  uStack_14c = uVar11;
  if (param_4 - 0x2d1U < 2) {
    if (param_4 == 0x2d2) {
      alStack_140[0] = CONCAT44(alStack_140[0]._4_4_,0x10);
      iVar4 = 0x10;
    }
    FUN_140898570(param_1,auStack_68,param_3,0x2ce,CONCAT44(uVar18,1),iVar4 + param_6,param_7,
                  param_8);
    FUN_140898570(param_1,auStack_58,param_3,0x420,1,iVar4 + param_6,param_7,param_8);
    FUN_140879000(&iStack_158,param_2);
    return param_2;
  }
  lVar7 = 0;
  alStack_140[0] = 0;
  if (param_3 < 0) {
    uStack_144 = -1;
LAB_140898665:
    if (param_4 - 0x29fU < 7) {
      alStack_140[0] =
           *(longlong *)(*(longlong *)(param_1 + 0x2d7f8) + 0x18 + (longlong)(int)uStack_144 * 8);
    }
    else {
      FUN_1407fba90(&iStack_130,param_1);
      lVar7 = CONCAT44(uStack_12c,iStack_130);
    }
    if ((-1 < param_7) &&
       (param_7 < (int)(*(longlong *)(lVar7 + 0x11378) - *(longlong *)(lVar7 + 0x11370) >> 3))) {
      FUN_1407046a0(alStack_140,param_1,param_3,param_7);
      lVar13 = alStack_140[0];
      lVar7 = 0;
      uStack_148 = 0;
      if (((param_4 - 0x41fU < 10) || (param_4 == 0x2ce)) && (0xf < (int)param_6)) {
        uStack_148 = 1;
        param_6 = param_6 - 0x10;
      }
      uStack_144 = param_6;
      if (-1 < (int)param_6) {
        if ((int)param_6 < 0x10) {
          FUN_1405edd20(alStack_140[0] + 0x918);
        }
        if ((int)uStack_144 < 0x40) {
          FUN_1405edcc0(lVar13 + 0x9a0);
        }
      }
      if (uStack_144 < 8) {
        lVar12 = *(longlong *)(lVar13 + 0x8c8 + (longlong)(int)uStack_144 * 8);
        if ((lVar12 == 0) || (*(int *)(lVar12 + 8) == 0)) {
          bVar3 = false;
        }
        else {
          bVar3 = true;
        }
        if (bVar3) {
          lVar7 = *(longlong *)(lVar13 + 0x8c8 + (longlong)(int)uStack_144 * 8);
        }
      }
      if (param_4 - 2U < 0x429) {
                    /* WARNING: Could not recover jumptable at 0x0001408987c6. Too many branches */
                    /* WARNING: Treating indirect jump as call */
        puVar5 = (undefined8 *)
                 (*(code *)(IMAGE_DOS_HEADER_140000000.e_magic +
                           *(uint *)((longlong)(int)(param_4 - 2U) * 4 + 0x1408a42f0)))
                           (IMAGE_DOS_HEADER_140000000.e_magic +
                            *(uint *)((longlong)(int)(param_4 - 2U) * 4 + 0x1408a42f0));
        return puVar5;
      }
      if (lVar7 != 0) {
        uVar6 = FUN_14098f930();
        FUN_14098efc0(uVar6,&uStack_128,param_4,lVar7,&iStack_158);
        if (cStack_118 != '\0') {
          *param_2 = uStack_128;
          param_2[1] = uStack_120;
          return param_2;
        }
      }
      goto LAB_1408a4207;
    }
  }
  else {
    uStack_144 = (int)(param_3 + (param_3 >> 0x1f & 0x7fU)) >> 7;
    if (param_3 < 0x2000) goto LAB_140898665;
  }
  lVar13 = 0;
  lVar12 = 0;
  lVar16 = 0;
  if (param_3 < 0x2000) {
    if (param_8 == 0xffffffff) {
      uVar10 = 0xffffffff;
    }
    else {
      uVar10 = param_8 & 0xffff;
    }
    lVar9 = (longlong)(int)param_6;
    lVar14 = lVar13;
    lVar17 = lVar13;
    if ((-1 < (int)param_6) && (lVar14 = lVar12, lVar17 = lVar16, (int)param_6 < 8)) {
      if (lVar7 != 0) {
        if ((int)uVar10 < 2) {
          lVar13 = *(longlong *)(lVar7 + 0x1f5f0 + lVar9 * 8);
          if ((lVar13 == 0) || (*(int *)(lVar13 + 8) == 0)) {
            bVar3 = false;
          }
          else {
            bVar3 = true;
          }
          if (!bVar3) goto LAB_14089e97e;
          lVar13 = *(longlong *)(lVar7 + 0x1f5f0 + lVar9 * 8);
        }
        else {
LAB_14089e97e:
          lVar13 = lVar12;
          if (uVar10 - 3 < 0x10) {
            lVar12 = *(longlong *)
                      (lVar7 + 0x1d068 + ((longlong)(int)(uVar10 - 3) * 0x4b + lVar9) * 8);
            if ((lVar12 == 0) || (*(int *)(lVar12 + 8) == 0)) {
              bVar3 = false;
            }
            else {
              bVar3 = true;
            }
            if (bVar3) {
              lVar13 = *(longlong *)
                        (lVar7 + 0x1d068 + ((longlong)(int)(uVar10 - 3) * 0x4b + lVar9) * 8);
            }
          }
        }
        if (uVar10 == 0x14) {
          lVar12 = *(longlong *)(lVar7 + 0x1f9a8 + lVar9 * 8);
          if ((lVar12 == 0) || (*(int *)(lVar12 + 8) == 0)) {
            bVar3 = false;
          }
          else {
            bVar3 = true;
          }
          if (bVar3) {
            lVar13 = *(longlong *)(lVar7 + 0x1f9a8 + lVar9 * 8);
          }
        }
      }
      lVar14 = lVar13;
      if (((int)param_6 < 8) && (lVar7 != 0)) {
        lVar13 = *(longlong *)(lVar7 + 0x1f768 + lVar9 * 8);
        if ((lVar13 == 0) || (*(int *)(lVar13 + 8) == 0)) {
          bVar3 = false;
        }
        else {
          bVar3 = true;
        }
        if (bVar3) {
          lVar17 = *(longlong *)(lVar7 + 0x1f768 + lVar9 * 8);
        }
      }
    }
    if ((((uVar10 == 1) || (uVar10 == 0x14)) || (uVar10 - 3 < 0x10)) &&
       ((param_4 - 0x214U < 0x7f || (param_4 - 0x3c9U < 0x27)))) goto LAB_14089ea89;
  }
  else {
    lVar14 = *(longlong *)(param_1 + 0x2d7f8) + 0x1820 +
             (longlong)(param_3 + -0x2000) * 0x52a0 + (longlong)(int)param_6 * 0x1470;
LAB_14089ea89:
    lVar17 = lVar14;
  }
  iStack_130 = FUN_140990370(param_4,param_6);
  if (iStack_130 < 0) {
    iStack_130 = FUN_140993fc0(param_4,param_6);
    if (iStack_130 < 0) {
      iVar4 = FUN_1409940f0(param_4,param_6);
      if (iVar4 < 0) {
        iVar4 = FUN_140994410(param_4,param_6);
        if (-1 < iVar4) {
          lVar7 = FUN_1408ab280(param_1,auStack_78,param_3,param_6,CONCAT44(uVar18,0xd),iVar4,1,
                                param_8);
          uVar18 = *(undefined4 *)(lVar7 + 8);
          *(undefined4 *)param_2 = 0;
          *(undefined4 *)(param_2 + 1) = uVar18;
          return param_2;
        }
        if (param_4 - 0x13U < 0x418) {
                    /* WARNING: Could not recover jumptable at 0x00014089ee71. Too many branches */
                    /* WARNING: Treating indirect jump as call */
          puVar5 = (undefined8 *)
                   (*(code *)(IMAGE_DOS_HEADER_140000000.e_magic +
                             *(uint *)((longlong)(int)(param_4 - 0x13U) * 4 + 0x1408a5394)))
                             (IMAGE_DOS_HEADER_140000000.e_magic +
                              *(uint *)((longlong)(int)(param_4 - 0x13U) * 4 + 0x1408a5394));
          return puVar5;
        }
        if (param_8 == 0) {
          lVar14 = lVar17;
        }
        if (lVar14 != 0) {
          uVar6 = FUN_14098f930();
          FUN_14098efc0(uVar6,&uStack_110,param_4,lVar14,&iStack_158);
          if (cStack_100 != '\0') {
            *param_2 = uStack_110;
            param_2[1] = uStack_108;
            return param_2;
          }
        }
LAB_1408a4207:
        if (param_5 == 3) {
          DAT_14a601000 = 0;
          *(undefined4 *)param_2 = 4;
          param_2[1] = &DAT_14a601000;
          return param_2;
        }
        if (param_5 != 4) {
          *(undefined4 *)param_2 = 0;
          *(undefined4 *)(param_2 + 1) = 0;
          return param_2;
        }
        *(undefined4 *)param_2 = 3;
        *(undefined1 *)(param_2 + 1) = 1;
        return param_2;
      }
      lVar7 = FUN_1408ab280(param_1,auStack_88,param_3,param_6,CONCAT44(uVar18,0xc),iVar4,1,0);
      iVar4 = *(int *)(lVar7 + 8);
      *(undefined4 *)param_2 = 0;
    }
    else {
      lVar7 = FUN_1408ab280(param_1,auStack_b8,param_3,param_6,CONCAT44(uVar18,9),iStack_130,0x15,0)
      ;
      iVar4 = iStack_130;
      iVar1 = *(int *)(lVar7 + 8);
      lVar7 = FUN_1408ab280(param_1,auStack_a8,param_3,param_6,9,iStack_130,0x16,0);
      iVar2 = *(int *)(lVar7 + 8);
      lVar7 = FUN_1408ab280(param_1,auStack_98,param_3,param_6,9,iVar4,1,0);
      iVar4 = *(int *)(lVar7 + 8);
      *(undefined4 *)param_2 = 0;
      if (param_5 != 1) {
        if (param_5 == 2) {
          *(float *)(param_2 + 1) = ((float)iVar4 - (float)iVar1) / ((float)iVar2 - (float)iVar1);
          return param_2;
        }
        *(float *)(param_2 + 1) = (float)iVar4;
        return param_2;
      }
    }
    *(float *)(param_2 + 1) = (float)iVar4;
    return param_2;
  }
  lVar7 = FUN_1408ab280(param_1,auStack_f8,param_3,param_6,CONCAT44(uVar18,2),iStack_130,0x15,0);
  iVar2 = iStack_130;
  iVar4 = *(int *)(lVar7 + 8);
  lVar7 = FUN_1408ab280(param_1,auStack_e8,param_3,param_6,2,iStack_130,0x16,0);
  iStack_130 = *(int *)(lVar7 + 8);
  lVar7 = FUN_1408ab280(param_1,auStack_d8,param_3,param_6,2,iVar2,1,0);
  iVar1 = *(int *)(lVar7 + 8);
  if (param_5 == 1) {
    *(undefined4 *)param_2 = 0;
    *(float *)(param_2 + 1) = (float)iVar1;
    return param_2;
  }
  if (param_5 == 2) {
    *(undefined4 *)param_2 = 0;
    *(float *)(param_2 + 1) = ((float)iVar1 - (float)iVar4) / ((float)iStack_130 - (float)iVar4);
    return param_2;
  }
  piVar8 = (int *)FUN_1408ab280(param_1,auStack_c8,param_3,param_6,2,iVar2,0xe,0);
  if (*piVar8 == 4) {
    pcVar15 = *(char **)(piVar8 + 2);
    if (pcVar15 == (char *)0x0) goto LAB_14089ec2c;
  }
  else {
    pcVar15 = (char *)0x14a601270;
  }
  if (*pcVar15 != '\0') {
    FUN_1405236e0(0x14a601130,0x140,0x14470c660);
    *(undefined4 *)param_2 = 4;
    param_2[1] = 0x14a601130;
    return param_2;
  }
LAB_14089ec2c:
  FUN_1405236e0(0x14a601130,0x140,0x144709610,iVar1);
  *(undefined4 *)param_2 = 4;
  param_2[1] = 0x14a601130;
  return param_2;
}



/* VA 1409ab730 */

/* WARNING: Globals starting with '_' overlap smaller symbols at the same address */

void FUN_1409ab730(longlong param_1)

{
  longlong lVar1;
  uint uVar2;
  int iVar3;
  uint uVar4;
  int iVar5;
  ulonglong uVar6;
  uint uVar7;
  int iVar8;
  int iVar9;
  longlong lVar10;
  longlong lVar11;
  int iVar12;

  iVar9 = *(int *)(param_1 + 0x1cf30) - *(int *)(param_1 + 0x1cf34);
  if (*(int *)(param_1 + 0x1cf08) == -1) {
    uVar6 = 0x60;
    if (0 < (int)*(uint *)(param_1 + 0x1cf14)) {
      uVar6 = (ulonglong)*(uint *)(param_1 + 0x1cf14);
    }
    iVar3 = (int)(*(uint *)(param_1 + 0x1cef0) / uVar6);
  }
  else {
    iVar3 = *(int *)(param_1 + 0x1cf0c);
  }
  iVar12 = 0;
  if (0 < iVar3) {
    do {
      iVar5 = 0x60;
      if (0 < *(int *)(param_1 + 0x1cf14)) {
        iVar5 = *(int *)(param_1 + 0x1cf14);
      }
      lVar11 = (longlong)(iVar5 * iVar12) + *(longlong *)(param_1 + 0x1cee8);
      if ((*(int *)(lVar11 + 0x28) == 2) && (*(char *)(lVar11 + 8) == '\0')) {
        uVar7 = _DAT_144fd4c80 - 1;
        uVar4 = (uint)*(ushort *)(lVar11 + 0x32) + iVar9;
        uVar2 = 0;
        if (-1 < (int)uVar4) {
          uVar2 = uVar4;
        }
        if ((int)uVar2 <= (int)uVar7) {
          uVar7 = uVar2 & 0xffff;
        }
        *(short *)(lVar11 + 0x32) = (short)uVar7;
      }
      iVar12 = iVar12 + 1;
    } while (iVar12 < iVar3);
  }
  lVar11 = *(longlong *)(param_1 + 0x11370);
  iVar3 = (int)(*(longlong *)(param_1 + 0x11378) - lVar11 >> 3);
  if (0 < iVar3) {
    lVar10 = 0;
    do {
      lVar1 = *(longlong *)(lVar11 + lVar10 * 8);
      if (*(int *)(lVar1 + 0x108) == -1) {
        uVar6 = 0x60;
        if (0 < (int)*(uint *)(lVar1 + 0x114)) {
          uVar6 = (ulonglong)*(uint *)(lVar1 + 0x114);
        }
        lVar11 = *(longlong *)(param_1 + 0x11370);
        iVar12 = (int)(*(uint *)(lVar1 + 0xf0) / uVar6);
      }
      else {
        iVar12 = *(int *)(lVar1 + 0x10c);
      }
      iVar5 = 0;
      if (0 < iVar12) {
        do {
          iVar8 = 0x60;
          if (0 < *(int *)(lVar1 + 0x114)) {
            iVar8 = *(int *)(lVar1 + 0x114);
          }
          lVar11 = (longlong)(iVar8 * iVar5) + *(longlong *)(lVar1 + 0xe8);
          if ((*(int *)(lVar11 + 0x28) == 2) && (*(char *)(lVar11 + 8) == '\0')) {
            uVar7 = _DAT_144fd4c80 - 1;
            uVar4 = (uint)*(ushort *)(lVar11 + 0x32) + iVar9;
            uVar2 = 0;
            if (-1 < (int)uVar4) {
              uVar2 = uVar4;
            }
            if ((int)uVar2 <= (int)uVar7) {
              uVar7 = uVar2 & 0xffff;
            }
            *(short *)(lVar11 + 0x32) = (short)uVar7;
          }
          iVar5 = iVar5 + 1;
        } while (iVar5 < iVar12);
        lVar11 = *(longlong *)(param_1 + 0x11370);
      }
      lVar10 = lVar10 + 1;
    } while (lVar10 < iVar3);
  }
  return;
}
