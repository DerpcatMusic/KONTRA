
void analyst_entry_8af9e0(undefined8 *param_1,undefined4 param_2)

{
  undefined4 uVar1;
  undefined8 *puVar2;
  ulonglong uVar3;
  undefined8 local_a8 [4];
  undefined8 local_88;
  undefined8 uStack_80;
  undefined8 local_78;
  undefined8 uStack_70;
  undefined8 local_68;
  undefined8 uStack_60;
  undefined8 local_58;
  undefined8 uStack_50;
  undefined8 local_48;
  undefined8 uStack_40;
  undefined8 local_38;
  undefined8 uStack_30;
  uint local_28;
  undefined4 uStack_24;
  undefined4 uStack_20;
  undefined4 uStack_1c;
  
                    /* 0x8af9e0  1  analyst_entry_8af9e0 */
  local_a8[0] = *param_1;
  local_a8[1] = param_1[1];
  local_88 = param_1[4];
  uStack_80 = param_1[5];
  local_a8[2] = param_1[2];
  local_a8[3] = param_1[3];
  local_68 = param_1[8];
  uStack_60 = param_1[9];
  local_78 = param_1[6];
  uStack_70 = param_1[7];
  local_58 = param_1[10];
  uStack_50 = param_1[0xb];
  local_48 = param_1[0xc];
  uStack_40 = param_1[0xd];
  local_28 = *(uint *)(param_1 + 0x10);
  uVar1 = *(undefined4 *)((longlong)param_1 + 0x84);
  local_38 = param_1[0xe];
  uStack_30 = param_1[0xf];
  uStack_24 = uVar1;
  uStack_20 = *(undefined4 *)(param_1 + 0x11);
  uStack_1c = *(undefined4 *)((longlong)param_1 + 0x8c);
  if (0 < (int)local_28) {
    puVar2 = local_a8;
    uVar3 = (ulonglong)local_28;
    do {
      func_0x000180001040(param_2,*puVar2,uVar1);
      puVar2 = puVar2 + 1;
      uVar3 = uVar3 - 1;
    } while (uVar3 != 0);
  }
  return;
}

