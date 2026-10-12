; Original instructions; Kontakt 8.13.1 standalone SHA256 0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8

; Span 0x140974c6a..0x140974c86
140974c6a  48638258050000           movsxd rax, dword ptr [rdx + 0x558]
140974c71  83f806                   cmp eax, 6
140974c74  0f8762160000             ja 0x1409762dc
140974c7a  8b8c839c869700           mov ecx, dword ptr [rbx + rax*4 + 0x97869c]
140974c81  4803cb                   add rcx, rbx
140974c84  ffe1                     jmp rcx

; Span 0x140975131..0x140975158
140975131  8b8208030000             mov eax, dword ptr [rdx + 0x308]
140975137  ffc8                     dec eax
140975139  83f80f                   cmp eax, 0xf
14097513c  0f879a110000             ja 0x1409762dc
140975142  4898                     cdqe 
140975144  8b8c83f8879700           mov ecx, dword ptr [rbx + rax*4 + 0x9787f8]
14097514b  4803cb                   add rcx, rbx
14097514e  ffe1                     jmp rcx
140975150  498bce                   mov rcx, r14
140975153  e8b8dbe4ff               call 0x1407c2d10

; Span 0x140975220..0x140975247
140975220  8b8208030000             mov eax, dword ptr [rdx + 0x308]
140975226  ffc8                     dec eax
140975228  83f80f                   cmp eax, 0xf
14097522b  0f87ab100000             ja 0x1409762dc
140975231  4898                     cdqe 
140975233  8b8c8338889700           mov ecx, dword ptr [rbx + rax*4 + 0x978838]
14097523a  4803cb                   add rcx, rbx
14097523d  ffe1                     jmp rcx
14097523f  498bce                   mov rcx, r14
140975242  e8c9cee4ff               call 0x1407c2110

; Span 0x1407c2110..0x1407c21c0
1407c2110  48895c2418               mov qword ptr [rsp + 0x18], rbx
1407c2115  57                       push rdi
1407c2116  4883ec30                 sub rsp, 0x30
1407c211a  488bd9                   mov rbx, rcx
1407c211d  80790c00                 cmp byte ptr [rcx + 0xc], 0
1407c2121  740a                     je 0x1407c212d
1407c2123  48833900                 cmp qword ptr [rcx], 0
1407c2127  0f8593000000             jne 0x1407c21c0
1407c212d  bac8010000               mov edx, 0x1c8
1407c2132  488b0d2f77df09           mov rcx, qword ptr [rip + 0x9df772f]
1407c2139  e8d2f8f4ff               call 0x140711a10
1407c213e  488bf8                   mov rdi, rax
1407c2141  4885c0                   test rax, rax
1407c2144  7470                     je 0x1407c21b6
1407c2146  4889442440               mov qword ptr [rsp + 0x40], rax
1407c214b  33d2                     xor edx, edx
1407c214d  41b8c8010000             mov r8d, 0x1c8
1407c2153  488bc8                   mov rcx, rax
1407c2156  e8a5eec003               call 0x1443d1000
1407c215b  488bcf                   mov rcx, rdi
1407c215e  e8cd9be1ff               call 0x1405dbd30
1407c2163  33c0                     xor eax, eax
1407c2165  898710010000             mov dword ptr [rdi + 0x110], eax
1407c216b  48898708010000           mov qword ptr [rdi + 0x108], rax
1407c2172  488d05c779f603           lea rax, [rip + 0x3f679c7]
1407c2179  488907                   mov qword ptr [rdi], rax
1407c217c  488d8f40010000           lea rcx, [rdi + 0x140]
1407c2183  48894c2448               mov qword ptr [rsp + 0x48], rcx
1407c2188  488d0561b7e1ff           lea rax, [rip - 0x1e489f]
1407c218f  4889442420               mov qword ptr [rsp + 0x20], rax
1407c2194  4c8d0d059ce1ff           lea r9, [rip - 0x1e63fb]
1407c219b  ba88000000               mov edx, 0x88
1407c21a0  41b801000000             mov r8d, 1
1407c21a6  e8a96ec003               call 0x1443c9054
1407c21ab  90                       nop 
1407c21ac  488d05557af603           lea rax, [rip + 0x3f67a55]
1407c21b3  488907                   mov qword ptr [rdi], rax
1407c21b6  48893b                   mov qword ptr [rbx], rdi
1407c21b9  c74308c8010000           mov dword ptr [rbx + 8], 0x1c8

; Span 0x14097411c..0x140974124
14097411c  f30f103d64455c04         movss xmm7, dword ptr [rip + 0x45c4564]

; Span 0x140976457..0x140976473
140976457  498b0e                   mov rcx, qword ptr [r14]
14097645a  488b01                   mov rax, qword ptr [rcx]
14097645d  488b5008                 mov rdx, qword ptr [rax + 8]
140976461  488b4608                 mov rax, qword ptr [rsi + 8]
140976465  f30f1088007d0200         movss xmm1, dword ptr [rax + 0x27d00]
14097646d  f30f59cf                 mulss xmm1, xmm7
140976471  ffd2                     call rdx

; Span 0x14072b380..0x14072b39e
14072b380  4883ec38                 sub rsp, 0x38
14072b384  f30f104134               movss xmm0, dword ptr [rcx + 0x34]
14072b389  f30f105930               movss xmm3, dword ptr [rcx + 0x30]
14072b38e  f30f11442420             movss dword ptr [rsp + 0x20], xmm0
14072b394  e857983400               call 0x140a74bf0
14072b399  4883c438                 add rsp, 0x38
14072b39d  c3                       ret 

; Span 0x140a74bf0..0x140a74bfa
140a74bf0  80790800                 cmp byte ptr [rcx + 8], 0
140a74bf4  7504                     jne 0x140a74bfa
140a74bf6  0f28c1                   movaps xmm0, xmm1
140a74bf9  c3                       ret 

; Span 0x140ab5ddc..0x140ab5df6
140ab5ddc  f30f10052419c803         movss xmm0, dword ptr [rip + 0x3c81924]
140ab5de4  0f2f4718                 comiss xmm0, dword ptr [rdi + 0x18]
140ab5de8  488b5c2430               mov rbx, qword ptr [rsp + 0x30]
140ab5ded  884709                   mov byte ptr [rdi + 9], al
140ab5df0  0f92c0                   setb al
140ab5df3  884708                   mov byte ptr [rdi + 8], al

; Span 0x140977d3a..0x140977d85
140977d3a  488b87a8050000           mov rax, qword ptr [rdi + 0x5a8]
140977d41  4c8b4028                 mov r8, qword ptr [rax + 0x28]
140977d45  488b4e08                 mov rcx, qword ptr [rsi + 8]
140977d49  488b81001c0100           mov rax, qword ptr [rcx + 0x11c00]
140977d50  4885c0                   test rax, rax
140977d53  7407                     je 0x140977d5c
140977d55  f30f105008               movss xmm2, dword ptr [rax + 8]
140977d5a  eb17                     jmp 0x140977d73
140977d5c  488b81f8d70200           mov rax, qword ptr [rcx + 0x2d7f8]
140977d63  660f6e9088110000         movd xmm2, dword ptr [rax + 0x1188]
140977d6b  0f5bd2                   cvtdq2ps xmm2, xmm2
140977d6e  f3410f59d0               mulss xmm2, xmm8
140977d73  f30f108f5c050000         movss xmm1, dword ptr [rdi + 0x55c]
140977d7b  488d8fa8050000           lea rcx, [rdi + 0x5a8]
140977d82  41ffd0                   call r8

; Span 0x14097741e..0x140977427
14097741e  f3440f10057dd4d903       movss xmm8, dword ptr [rip + 0x3d9d47d]

; Span 0x140977442..0x14097744b
140977442  f2440f100d3dd5d903       movsd xmm9, qword ptr [rip + 0x3d9d53d]

; Span 0x140977d73..0x140977de0
140977d73  f30f108f5c050000         movss xmm1, dword ptr [rdi + 0x55c]
140977d7b  488d8fa8050000           lea rcx, [rdi + 0x5a8]
140977d82  41ffd0                   call r8
140977d85  f30f594314               mulss xmm0, dword ptr [rbx + 0x14]
140977d8a  f3410f59c0               mulss xmm0, xmm8
140977d8f  f30f2cc0                 cvttss2si eax, xmm0
140977d93  8983f8000000             mov dword ptr [rbx + 0xf8], eax
140977d99  660f6ec0                 movd xmm0, eax
140977d9d  f30fe6c0                 cvtdq2pd xmm0, xmm0
140977da1  410f28c9                 movaps xmm1, xmm9
140977da5  f20f5ec8                 divsd xmm1, xmm0
140977da9  f20f1005bff9db03         movsd xmm0, qword ptr [rip + 0x3dbf9bf]
140977db1  e8f892a503               call 0x1443d10ae
140977db6  0f57c9                   xorps xmm1, xmm1
140977db9  f20f5ac8                 cvtsd2ss xmm1, xmm0
140977dbd  f30f118bf4000000         movss dword ptr [rbx + 0xf4], xmm1
140977dc5  f30f109768050000         movss xmm2, dword ptr [rdi + 0x568]
140977dcd  f30f1193e0000000         movss dword ptr [rbx + 0xe0], xmm2
140977dd5  0f5ac2                   cvtps2pd xmm0, xmm2
140977dd8  f20f1183e8000000         movsd qword ptr [rbx + 0xe8], xmm0

; Span 0x140977de0..0x140977df3
140977de0  0fb6876d050000           movzx eax, byte ptr [rdi + 0x56d]
140977de7  8883e5000000             mov byte ptr [rbx + 0xe5], al
140977ded  488b03                   mov rax, qword ptr [rbx]
140977df0  488bcb                   mov rcx, rbx

; Span 0x140977e45..0x140977eb9
140977e45  8b8758050000             mov eax, dword ptr [rdi + 0x558]
140977e4b  83e805                   sub eax, 5
140977e4e  83f801                   cmp eax, 1
140977e51  0f87effdffff             ja 0x140977c46
140977e57  f30f10a7f0050000         movss xmm4, dword ptr [rdi + 0x5f0]
140977e5f  f30f109fec050000         movss xmm3, dword ptr [rdi + 0x5ec]
140977e67  f30f1097e8050000         movss xmm2, dword ptr [rdi + 0x5e8]
140977e6f  f30f108fe4050000         movss xmm1, dword ptr [rdi + 0x5e4]
140977e77  f30f1087e0050000         movss xmm0, dword ptr [rdi + 0x5e0]
140977e7f  f30f118328010000         movss dword ptr [rbx + 0x128], xmm0
140977e87  f30f118b34010000         movss dword ptr [rbx + 0x134], xmm1
140977e8f  f30f119330010000         movss dword ptr [rbx + 0x130], xmm2
140977e97  f30f119b2c010000         movss dword ptr [rbx + 0x12c], xmm3
140977e9f  f30f11a338010000         movss dword ptr [rbx + 0x138], xmm4
140977ea7  0fb6876c050000           movzx eax, byte ptr [rdi + 0x56c]
140977eae  88833c010000             mov byte ptr [rbx + 0x13c], al
140977eb4  e98dfdffff               jmp 0x140977c46
