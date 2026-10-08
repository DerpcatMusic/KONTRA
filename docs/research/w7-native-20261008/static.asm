
# symbol_registration: 0x14045ab80 bytes=112 sha256=b1f533f2318f5cd240b63d505462cb0a4a4cd3174f6c7ea025ed3bba149e49cb
000000014045ab80  c7852c01000056020000    mov      dword ptr [rbp + 0x12c], 0x256
000000014045ab8a  4c8d852c010000          lea      r8, [rbp + 0x12c]
000000014045ab91  488d15f02ba204          lea      rdx, [rip + 0x4a22bf0]
000000014045ab98  488d8d20300000          lea      rcx, [rbp + 0x3020]
000000014045ab9f  e84cb33100              call     0x140775ef0
000000014045aba4  90                      nop
000000014045aba5  c7853001000057020000    mov      dword ptr [rbp + 0x130], 0x257
000000014045abaf  4c8d8530010000          lea      r8, [rbp + 0x130]
000000014045abb6  488d15eb2ba204          lea      rdx, [rip + 0x4a22beb]
000000014045abbd  488d8d48300000          lea      rcx, [rbp + 0x3048]
000000014045abc4  e827b33100              call     0x140775ef0
000000014045abc9  90                      nop
000000014045abca  c7853401000058020000    mov      dword ptr [rbp + 0x134], 0x258
000000014045abd4  4c8d8534010000          lea      r8, [rbp + 0x134]
000000014045abdb  488d15e62ba204          lea      rdx, [rip + 0x4a22be6]
000000014045abe2  488d8d70300000          lea      rcx, [rbp + 0x3070]
000000014045abe9  e802b33100              call     0x140775ef0
000000014045abee  90                      nop

# join_menu: 0x14061c1a2 bytes=82 sha256=ca5bb59d08fac7aa8e17447091e9e7d3ea9296005345254219aba9465c7227a6
000000014061c1a2  4533c9                  xor      r9d, r9d
000000014061c1a5  4c8d05cfb60e04          lea      r8, [rip + 0x40eb6cf]
000000014061c1ac  488d1595b61004          lea      rdx, [rip + 0x410b695]
000000014061c1b3  488bc8                  mov      rcx, rax
000000014061c1b6  488bd8                  mov      rbx, rax
000000014061c1b9  e8f2a38d01              call     0x141ef65b0
000000014061c1be  41b902000000            mov      r9d, 2
000000014061c1c4  4c8d05b0b60e04          lea      r8, [rip + 0x40eb6b0]
000000014061c1cb  488d157ab61004          lea      rdx, [rip + 0x410b67a]
000000014061c1d2  488bcb                  mov      rcx, rbx
000000014061c1d5  e8d6a38d01              call     0x141ef65b0
000000014061c1da  41b901000000            mov      r9d, 1
000000014061c1e0  4c8d0594b60e04          lea      r8, [rip + 0x40eb694]
000000014061c1e7  488d1562b61004          lea      rdx, [rip + 0x410b662]
000000014061c1ee  488bcb                  mov      rcx, rbx

# join_diagnostic: 0x140878025 bytes=122 sha256=5acab69e75aab27059d5a7bb44deb1e98dd817b572b0ee621028c2cb827b8d02
0000000140878025  41b701                  mov      r15b, 1
0000000140878028  498bce                  mov      rcx, r14
000000014087802b  4c8b5598                mov      r10, qword ptr [rbp - 0x68]
000000014087802f  4983fe04                cmp      r14, 4
0000000140878033  7d6a                    jge      0x14087809f
0000000140878035  498d82c8070000          lea      rax, [r10 + 0x7c8]
000000014087803c  4803c6                  add      rax, rsi
000000014087803f  90                      nop
0000000140878040  833800                  cmp      dword ptr [rax], 0
0000000140878043  750f                    jne      0x140878054
0000000140878045  48ffc1                  inc      rcx
0000000140878048  4883c030                add      rax, 0x30
000000014087804c  4883f904                cmp      rcx, 4
0000000140878050  7cee                    jl       0x140878040
0000000140878052  eb4b                    jmp      0x14087809f
0000000140878054  428b8c16b8070000        mov      ecx, dword ptr [rsi + r10 + 0x7b8]
000000014087805c  85c9                    test     ecx, ecx
000000014087805e  741c                    je       0x14087807c
0000000140878060  83e901                  sub      ecx, 1
0000000140878063  740e                    je       0x140878073
0000000140878065  83f901                  cmp      ecx, 1
0000000140878068  7523                    jne      0x14087808d
000000014087806a  488d15b7016a04          lea      rdx, [rip + 0x46a01b7]
0000000140878071  eb10                    jmp      0x140878083
0000000140878073  488d159e016a04          lea      rdx, [rip + 0x46a019e]
000000014087807a  eb07                    jmp      0x140878083
000000014087807c  488d1585016a04          lea      rdx, [rip + 0x46a0185]
0000000140878083  488d4c2430              lea      rcx, [rsp + 0x30]
0000000140878088  e833fbecff              call     0x140747bc0
000000014087808d  488d542430              lea      rdx, [rsp + 0x30]
0000000140878092  488d4dc0                lea      rcx, [rbp - 0x40]
0000000140878096  e8b566e9ff              call     0x14070e750
000000014087809b  4c8b5598                mov      r10, qword ptr [rbp - 0x68]

# criteria_reader: 0x140d04400 bytes=283 sha256=359c4269e0a4115a4bc0b6108def2c2101912bfebdf46195837417555981dfdf
0000000140d04400  48895c2408              mov      qword ptr [rsp + 8], rbx
0000000140d04405  57                      push     rdi
0000000140d04406  4883ec40                sub      rsp, 0x40
0000000140d0440a  488bfa                  mov      rdi, rdx
0000000140d0440d  488bd9                  mov      rbx, rcx
0000000140d04410  664183f870              cmp      r8w, 0x70
0000000140d04415  0f851c010000            jne      0x140d04537
0000000140d0441b  488bca                  mov      rcx, rdx
0000000140d0441e  e84d3ed701              call     0x142a78270
0000000140d04423  83f805                  cmp      eax, 5
0000000140d04426  0f87ef000000            ja       0x140d0451b
0000000140d0442c  488d15cdbb2fff          lea      rdx, [rip - 0xd04433]
0000000140d04433  4898                    cdqe
0000000140d04435  8b8c827045d000          mov      ecx, dword ptr [rdx + rax*4 + 0xd04570]
0000000140d0443c  4803ca                  add      rcx, rdx
0000000140d0443f  ffe1                    jmp      rcx
0000000140d04441  c7430800000000          mov      dword ptr [rbx + 8], 0
0000000140d04448  eb2b                    jmp      0x140d04475
0000000140d0444a  c7430801000000          mov      dword ptr [rbx + 8], 1
0000000140d04451  eb22                    jmp      0x140d04475
0000000140d04453  c7430802000000          mov      dword ptr [rbx + 8], 2
0000000140d0445a  eb19                    jmp      0x140d04475
0000000140d0445c  c7430803000000          mov      dword ptr [rbx + 8], 3
0000000140d04463  eb10                    jmp      0x140d04475
0000000140d04465  c7430804000000          mov      dword ptr [rbx + 8], 4
0000000140d0446c  eb07                    jmp      0x140d04475
0000000140d0446e  c7430805000000          mov      dword ptr [rbx + 8], 5
0000000140d04475  488bcf                  mov      rcx, rdi
0000000140d04478  e8f33dd701              call     0x142a78270
0000000140d0447d  85c0                    test     eax, eax
0000000140d0447f  7420                    je       0x140d044a1
0000000140d04481  83e801                  sub      eax, 1
0000000140d04484  7412                    je       0x140d04498
0000000140d04486  83f801                  cmp      eax, 1
0000000140d04489  0f85c4000000            jne      0x140d04553
0000000140d0448f  c7432802000000          mov      dword ptr [rbx + 0x28], 2
0000000140d04496  eb10                    jmp      0x140d044a8
0000000140d04498  c7432801000000          mov      dword ptr [rbx + 0x28], 1
0000000140d0449f  eb07                    jmp      0x140d044a8
0000000140d044a1  c7432800000000          mov      dword ptr [rbx + 0x28], 0
0000000140d044a8  488bcf                  mov      rcx, rdi
0000000140d044ab  e8703dd701              call     0x142a78220
0000000140d044b0  488bcf                  mov      rcx, rdi
0000000140d044b3  6689430c                mov      word ptr [rbx + 0xc], ax
0000000140d044b7  e8643dd701              call     0x142a78220
0000000140d044bc  488bcf                  mov      rcx, rdi
0000000140d044bf  6689430e                mov      word ptr [rbx + 0xe], ax
0000000140d044c3  e8583dd701              call     0x142a78220
0000000140d044c8  488bcf                  mov      rcx, rdi
0000000140d044cb  66894310                mov      word ptr [rbx + 0x10], ax
0000000140d044cf  e84c3dd701              call     0x142a78220
0000000140d044d4  488bcf                  mov      rcx, rdi
0000000140d044d7  66894312                mov      word ptr [rbx + 0x12], ax
0000000140d044db  e8403dd701              call     0x142a78220
0000000140d044e0  488bcf                  mov      rcx, rdi
0000000140d044e3  66894314                mov      word ptr [rbx + 0x14], ax
0000000140d044e7  e8843dd701              call     0x142a78270
0000000140d044ec  488bcf                  mov      rcx, rdi
0000000140d044ef  894324                  mov      dword ptr [rbx + 0x24], eax
0000000140d044f2  e8793dd701              call     0x142a78270
0000000140d044f7  488bcf                  mov      rcx, rdi
0000000140d044fa  894318                  mov      dword ptr [rbx + 0x18], eax
0000000140d044fd  e86e3dd701              call     0x142a78270
0000000140d04502  488bcf                  mov      rcx, rdi
0000000140d04505  89431c                  mov      dword ptr [rbx + 0x1c], eax
0000000140d04508  e8b33bd701              call     0x142a780c0
0000000140d0450d  884320                  mov      byte ptr [rbx + 0x20], al
0000000140d04510  488b5c2450              mov      rbx, qword ptr [rsp + 0x50]
0000000140d04515  4883c440                add      rsp, 0x40
0000000140d04519  5f                      pop      rdi
0000000140d0451a  c3                      ret

# criteria_writer: 0x140d12ec0 bytes=260 sha256=1d4cc70bf8acf3fb6684f68fcf9fbab79d349d5b0d07e1bc68a49c3448d1de4f
0000000140d12ec0  48895c2408              mov      qword ptr [rsp + 8], rbx
0000000140d12ec5  4889742410              mov      qword ptr [rsp + 0x10], rsi
0000000140d12eca  57                      push     rdi
0000000140d12ecb  4883ec40                sub      rsp, 0x40
0000000140d12ecf  48634108                movsxd   rax, dword ptr [rcx + 8]
0000000140d12ed3  488bf2                  mov      rsi, rdx
0000000140d12ed6  488bd9                  mov      rbx, rcx
0000000140d12ed9  83f805                  cmp      eax, 5
0000000140d12edc  0f87e6000000            ja       0x140d12fc8
0000000140d12ee2  488d0d17d12eff          lea      rcx, [rip - 0xd12ee9]
0000000140d12ee9  bf02000000              mov      edi, 2
0000000140d12eee  448b84810030d100        mov      r8d, dword ptr [rcx + rax*4 + 0xd13000]
0000000140d12ef6  4c03c1                  add      r8, rcx
0000000140d12ef9  41ffe0                  jmp      r8
0000000140d12efc  33d2                    xor      edx, edx
0000000140d12efe  eb1e                    jmp      0x140d12f1e
0000000140d12f00  ba01000000              mov      edx, 1
0000000140d12f05  eb17                    jmp      0x140d12f1e
0000000140d12f07  8bd7                    mov      edx, edi
0000000140d12f09  eb13                    jmp      0x140d12f1e
0000000140d12f0b  ba03000000              mov      edx, 3
0000000140d12f10  eb0c                    jmp      0x140d12f1e
0000000140d12f12  ba04000000              mov      edx, 4
0000000140d12f17  eb05                    jmp      0x140d12f1e
0000000140d12f19  ba05000000              mov      edx, 5
0000000140d12f1e  488bce                  mov      rcx, rsi
0000000140d12f21  e88aaed601              call     0x142a7ddb0
0000000140d12f26  8b4b28                  mov      ecx, dword ptr [rbx + 0x28]
0000000140d12f29  85c9                    test     ecx, ecx
0000000140d12f2b  7417                    je       0x140d12f44
0000000140d12f2d  83e901                  sub      ecx, 1
0000000140d12f30  740b                    je       0x140d12f3d
0000000140d12f32  83f901                  cmp      ecx, 1
0000000140d12f35  0f85a9000000            jne      0x140d12fe4
0000000140d12f3b  eb09                    jmp      0x140d12f46
0000000140d12f3d  bf01000000              mov      edi, 1
0000000140d12f42  eb02                    jmp      0x140d12f46
0000000140d12f44  33ff                    xor      edi, edi
0000000140d12f46  8bd7                    mov      edx, edi
0000000140d12f48  488bce                  mov      rcx, rsi
0000000140d12f4b  e860aed601              call     0x142a7ddb0
0000000140d12f50  0fb7530c                movzx    edx, word ptr [rbx + 0xc]
0000000140d12f54  488bce                  mov      rcx, rsi
0000000140d12f57  e814aed601              call     0x142a7dd70
0000000140d12f5c  0fb7530e                movzx    edx, word ptr [rbx + 0xe]
0000000140d12f60  488bce                  mov      rcx, rsi
0000000140d12f63  e808aed601              call     0x142a7dd70
0000000140d12f68  0fb75310                movzx    edx, word ptr [rbx + 0x10]
0000000140d12f6c  488bce                  mov      rcx, rsi
0000000140d12f6f  e8fcadd601              call     0x142a7dd70
0000000140d12f74  0fb75312                movzx    edx, word ptr [rbx + 0x12]
0000000140d12f78  488bce                  mov      rcx, rsi
0000000140d12f7b  e8f0add601              call     0x142a7dd70
0000000140d12f80  0fb75314                movzx    edx, word ptr [rbx + 0x14]
0000000140d12f84  488bce                  mov      rcx, rsi
0000000140d12f87  e8e4add601              call     0x142a7dd70
0000000140d12f8c  8b5324                  mov      edx, dword ptr [rbx + 0x24]
0000000140d12f8f  488bce                  mov      rcx, rsi
0000000140d12f92  e819aed601              call     0x142a7ddb0
0000000140d12f97  8b5318                  mov      edx, dword ptr [rbx + 0x18]
0000000140d12f9a  488bce                  mov      rcx, rsi
0000000140d12f9d  e80eaed601              call     0x142a7ddb0
0000000140d12fa2  8b531c                  mov      edx, dword ptr [rbx + 0x1c]
0000000140d12fa5  488bce                  mov      rcx, rsi
0000000140d12fa8  e803aed601              call     0x142a7ddb0
0000000140d12fad  0fb65320                movzx    edx, byte ptr [rbx + 0x20]
0000000140d12fb1  488bce                  mov      rcx, rsi
0000000140d12fb4  488b5c2450              mov      rbx, qword ptr [rsp + 0x50]
0000000140d12fb9  488b742458              mov      rsi, qword ptr [rsp + 0x58]
0000000140d12fbe  4883c440                add      rsp, 0x40
0000000140d12fc2  5f                      pop      rdi

# loop_reader: 0x140cff510 bytes=296 sha256=4adb863a010a761fa35131b46025519dfad9c34354574ee26c0491ebf661efd5
0000000140cff510  48895c2408              mov      qword ptr [rsp + 8], rbx
0000000140cff515  57                      push     rdi
0000000140cff516  4883ec40                sub      rsp, 0x40
0000000140cff51a  488bfa                  mov      rdi, rdx
0000000140cff51d  488bd9                  mov      rbx, rcx
0000000140cff520  664183f860              cmp      r8w, 0x60
0000000140cff525  0f858c000000            jne      0x140cff5b7
0000000140cff52b  488bca                  mov      rcx, rdx
0000000140cff52e  e83d8dd701              call     0x142a78270
0000000140cff533  85c0                    test     eax, eax
0000000140cff535  742a                    je       0x140cff561
0000000140cff537  83e801                  sub      eax, 1
0000000140cff53a  741c                    je       0x140cff558
0000000140cff53c  83e801                  sub      eax, 1
0000000140cff53f  740e                    je       0x140cff54f
0000000140cff541  83f801                  cmp      eax, 1
0000000140cff544  0f8589000000            jne      0x140cff5d3
0000000140cff54a  894320                  mov      dword ptr [rbx + 0x20], eax
0000000140cff54d  eb19                    jmp      0x140cff568
0000000140cff54f  c7432004000000          mov      dword ptr [rbx + 0x20], 4
0000000140cff556  eb10                    jmp      0x140cff568
0000000140cff558  c7432003000000          mov      dword ptr [rbx + 0x20], 3
0000000140cff55f  eb07                    jmp      0x140cff568
0000000140cff561  c7432000000000          mov      dword ptr [rbx + 0x20], 0
0000000140cff568  488bcf                  mov      rcx, rdi
0000000140cff56b  e8008dd701              call     0x142a78270
0000000140cff570  488bcf                  mov      rcx, rdi
0000000140cff573  894308                  mov      dword ptr [rbx + 8], eax
0000000140cff576  e8f58cd701              call     0x142a78270
0000000140cff57b  488bcf                  mov      rcx, rdi
0000000140cff57e  89430c                  mov      dword ptr [rbx + 0xc], eax
0000000140cff581  e8ea8cd701              call     0x142a78270
0000000140cff586  488bcf                  mov      rcx, rdi
0000000140cff589  894318                  mov      dword ptr [rbx + 0x18], eax
0000000140cff58c  e82f8bd701              call     0x142a780c0
0000000140cff591  488bcf                  mov      rcx, rdi
0000000140cff594  88431c                  mov      byte ptr [rbx + 0x1c], al
0000000140cff597  e8548bd701              call     0x142a780f0
0000000140cff59c  488bcf                  mov      rcx, rdi
0000000140cff59f  f30f114324              movss    dword ptr [rbx + 0x24], xmm0
0000000140cff5a4  e8c78cd701              call     0x142a78270
0000000140cff5a9  894328                  mov      dword ptr [rbx + 0x28], eax
0000000140cff5ac  488b5c2450              mov      rbx, qword ptr [rsp + 0x50]
0000000140cff5b1  4883c440                add      rsp, 0x40
0000000140cff5b5  5f                      pop      rdi
0000000140cff5b6  c3                      ret
0000000140cff5b7  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140cff5bc  e88fb5afff              call     0x1407fab50
0000000140cff5c1  488d15107b1309          lea      rdx, [rip + 0x9137b10]
0000000140cff5c8  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140cff5cd  e8041a6d03              call     0x1443d0fd6
0000000140cff5d2  cc                      int3
0000000140cff5d3  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140cff5d8  e8e37ed8ff              call     0x140a874c0
0000000140cff5dd  488d15ac7f1309          lea      rdx, [rip + 0x9137fac]
0000000140cff5e4  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140cff5e9  e8e8196d03              call     0x1443d0fd6
0000000140cff5ee  cc                      int3
0000000140cff5ef  cc                      int3
0000000140cff5f0  48895c2408              mov      qword ptr [rsp + 8], rbx
0000000140cff5f5  48896c2410              mov      qword ptr [rsp + 0x10], rbp
0000000140cff5fa  4889742420              mov      qword ptr [rsp + 0x20], rsi
0000000140cff5ff  57                      push     rdi
0000000140cff600  4881ece0000000          sub      rsp, 0xe0
0000000140cff607  488bea                  mov      rbp, rdx
0000000140cff60a  488bd9                  mov      rbx, rcx
0000000140cff60d  33f6                    xor      esi, esi
0000000140cff60f  664183f801              cmp      r8w, 1
0000000140cff614  0f85f3000000            jne      0x140cff70d
0000000140cff61a  488bca                  mov      rcx, rdx
0000000140cff61d  e89e8ad701              call     0x142a780c0
0000000140cff622  84c0                    test     al, al
0000000140cff624  0f84ca000000            je       0x140cff6f4
0000000140cff62a  488d4c2450              lea      rcx, [rsp + 0x50]
0000000140cff62f  e8fc6d7f01              call     0x1424f6430
0000000140cff634  90                      nop

# loop_writer: 0x140d0faa0 bytes=294 sha256=3043d1d253a34f65cc97c9ccaa4a8fedf3862e3322cb752eaf077f192f04348a
0000000140d0faa0  48895c2408              mov      qword ptr [rsp + 8], rbx
0000000140d0faa5  57                      push     rdi
0000000140d0faa6  4883ec40                sub      rsp, 0x40
0000000140d0faaa  448b4120                mov      r8d, dword ptr [rcx + 0x20]
0000000140d0faae  488bfa                  mov      rdi, rdx
0000000140d0fab1  488bd9                  mov      rbx, rcx
0000000140d0fab4  4585c0                  test     r8d, r8d
0000000140d0fab7  7427                    je       0x140d0fae0
0000000140d0fab9  4183e801                sub      r8d, 1
0000000140d0fabd  741a                    je       0x140d0fad9
0000000140d0fabf  4183e802                sub      r8d, 2
0000000140d0fac3  740d                    je       0x140d0fad2
0000000140d0fac5  4183f801                cmp      r8d, 1
0000000140d0fac9  756e                    jne      0x140d0fb39
0000000140d0facb  ba02000000              mov      edx, 2
0000000140d0fad0  eb10                    jmp      0x140d0fae2
0000000140d0fad2  ba01000000              mov      edx, 1
0000000140d0fad7  eb09                    jmp      0x140d0fae2
0000000140d0fad9  ba03000000              mov      edx, 3
0000000140d0fade  eb02                    jmp      0x140d0fae2
0000000140d0fae0  33d2                    xor      edx, edx
0000000140d0fae2  488bcf                  mov      rcx, rdi
0000000140d0fae5  e8c6e2d601              call     0x142a7ddb0
0000000140d0faea  8b5308                  mov      edx, dword ptr [rbx + 8]
0000000140d0faed  488bcf                  mov      rcx, rdi
0000000140d0faf0  e8bbe2d601              call     0x142a7ddb0
0000000140d0faf5  8b530c                  mov      edx, dword ptr [rbx + 0xc]
0000000140d0faf8  488bcf                  mov      rcx, rdi
0000000140d0fafb  e8b0e2d601              call     0x142a7ddb0
0000000140d0fb00  8b5318                  mov      edx, dword ptr [rbx + 0x18]
0000000140d0fb03  488bcf                  mov      rcx, rdi
0000000140d0fb06  e8a5e2d601              call     0x142a7ddb0
0000000140d0fb0b  0fb6531c                movzx    edx, byte ptr [rbx + 0x1c]
0000000140d0fb0f  488bcf                  mov      rcx, rdi
0000000140d0fb12  e889e0d601              call     0x142a7dba0
0000000140d0fb17  f30f104b24              movss    xmm1, dword ptr [rbx + 0x24]
0000000140d0fb1c  488bcf                  mov      rcx, rdi
0000000140d0fb1f  e8ace1d601              call     0x142a7dcd0
0000000140d0fb24  8b5328                  mov      edx, dword ptr [rbx + 0x28]
0000000140d0fb27  488bcf                  mov      rcx, rdi
0000000140d0fb2a  488b5c2450              mov      rbx, qword ptr [rsp + 0x50]
0000000140d0fb2f  4883c440                add      rsp, 0x40
0000000140d0fb33  5f                      pop      rdi
0000000140d0fb34  e977e2d601              jmp      0x142a7ddb0
0000000140d0fb39  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140d0fb3e  e87d79d7ff              call     0x140a874c0
0000000140d0fb43  488d15467a1209          lea      rdx, [rip + 0x9127a46]
0000000140d0fb4a  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140d0fb4f  e882146c03              call     0x1443d0fd6
0000000140d0fb54  cc                      int3
0000000140d0fb55  cc                      int3
0000000140d0fb56  cc                      int3
0000000140d0fb57  cc                      int3
0000000140d0fb58  cc                      int3
0000000140d0fb59  cc                      int3
0000000140d0fb5a  cc                      int3
0000000140d0fb5b  cc                      int3
0000000140d0fb5c  cc                      int3
0000000140d0fb5d  cc                      int3
0000000140d0fb5e  cc                      int3
0000000140d0fb5f  cc                      int3
0000000140d0fb60  48895c2408              mov      qword ptr [rsp + 8], rbx
0000000140d0fb65  4889742410              mov      qword ptr [rsp + 0x10], rsi
0000000140d0fb6a  57                      push     rdi
0000000140d0fb6b  4883ec70                sub      rsp, 0x70
0000000140d0fb6f  488bfa                  mov      rdi, rdx
0000000140d0fb72  488bf1                  mov      rsi, rcx
0000000140d0fb75  410fb6583d              movzx    ebx, byte ptr [r8 + 0x3d]
0000000140d0fb7a  0fb6d3                  movzx    edx, bl
0000000140d0fb7d  488bcf                  mov      rcx, rdi
0000000140d0fb80  e81be0d601              call     0x142a7dba0
0000000140d0fb85  84db                    test     bl, bl
0000000140d0fb87  7467                    je       0x140d0fbf0
0000000140d0fb89  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140d0fb8e  e89d727e01              call     0x1424f6e30
0000000140d0fb93  90                      nop
0000000140d0fb94  488d4e08                lea      rcx, [rsi + 8]
0000000140d0fb98  4c8d4c2420              lea      r9, [rsp + 0x20]
0000000140d0fb9d  4c8bc7                  mov      r8, rdi
0000000140d0fba0  488d942490000000        lea      rdx, [rsp + 0x90]
0000000140d0fba8  e8d3bd8001              call     0x14251b980
0000000140d0fbad  488d8ef8010000          lea      rcx, [rsi + 0x1f8]
0000000140d0fbb4  488bd7                  mov      rdx, rdi
0000000140d0fbb7  e8c4b58001              call     0x14251b180
0000000140d0fbbc  488b9e20020000          mov      rbx, qword ptr [rsi + 0x220]
0000000140d0fbc3  4885db                  test     rbx, rbx

# automation_reader: 0x140cfec70 bytes=355 sha256=c0b3a6aca45fd7fbfcb1e029e67bf7abaadd7cda95ad5ee9ab52b7da13352da5
0000000140cfec70  48895c2418              mov      qword ptr [rsp + 0x18], rbx
0000000140cfec75  4889742420              mov      qword ptr [rsp + 0x20], rsi
0000000140cfec7a  57                      push     rdi
0000000140cfec7b  4883ec40                sub      rsp, 0x40
0000000140cfec7f  488bd9                  mov      rbx, rcx
0000000140cfec82  498bf1                  mov      rsi, r9
0000000140cfec85  410fb7c8                movzx    ecx, r8w
0000000140cfec89  488bfa                  mov      rdi, rdx
0000000140cfec8c  83e970                  sub      ecx, 0x70
0000000140cfec8f  747d                    je       0x140cfed0e
0000000140cfec91  83f901                  cmp      ecx, 1
0000000140cfec94  0f8539010000            jne      0x140cfedd3
0000000140cfec9a  488d5328                lea      rdx, [rbx + 0x28]
0000000140cfec9e  4d8bc1                  mov      r8, r9
0000000140cfeca1  488bcf                  mov      rcx, rdi
0000000140cfeca4  e8b716ffff              call     0x140cf0360
0000000140cfeca9  488bcf                  mov      rcx, rdi
0000000140cfecac  e80f94d701              call     0x142a780c0
0000000140cfecb1  488bcf                  mov      rcx, rdi
0000000140cfecb4  884331                  mov      byte ptr [rbx + 0x31], al
0000000140cfecb7  e86495d701              call     0x142a78220
0000000140cfecbc  488bcf                  mov      rcx, rdi
0000000140cfecbf  66894332                mov      word ptr [rbx + 0x32], ax
0000000140cfecc3  e8a895d701              call     0x142a78270
0000000140cfecc8  488bcf                  mov      rcx, rdi
0000000140cfeccb  894340                  mov      dword ptr [rbx + 0x40], eax
0000000140cfecce  e89d95d701              call     0x142a78270
0000000140cfecd3  488bcf                  mov      rcx, rdi
0000000140cfecd6  894344                  mov      dword ptr [rbx + 0x44], eax
0000000140cfecd9  e81294d701              call     0x142a780f0
0000000140cfecde  488bcf                  mov      rcx, rdi
0000000140cfece1  f30f114350              movss    dword ptr [rbx + 0x50], xmm0
0000000140cfece6  e80594d701              call     0x142a780f0
0000000140cfeceb  488d5334                lea      rdx, [rbx + 0x34]
0000000140cfecef  f30f114354              movss    dword ptr [rbx + 0x54], xmm0
0000000140cfecf4  4c8bc6                  mov      r8, rsi
0000000140cfecf7  488bcf                  mov      rcx, rdi
0000000140cfecfa  488b5c2460              mov      rbx, qword ptr [rsp + 0x60]
0000000140cfecff  488b742468              mov      rsi, qword ptr [rsp + 0x68]
0000000140cfed04  4883c440                add      rsp, 0x40
0000000140cfed08  5f                      pop      rdi
0000000140cfed09  e9021affff              jmp      0x140cf0710
0000000140cfed0e  48896c2450              mov      qword ptr [rsp + 0x50], rbp
0000000140cfed13  488d5328                lea      rdx, [rbx + 0x28]
0000000140cfed17  4c8bc6                  mov      r8, rsi
0000000140cfed1a  4c89742458              mov      qword ptr [rsp + 0x58], r14
0000000140cfed1f  488bcf                  mov      rcx, rdi
0000000140cfed22  e83916ffff              call     0x140cf0360
0000000140cfed27  488bcf                  mov      rcx, rdi
0000000140cfed2a  e89193d701              call     0x142a780c0
0000000140cfed2f  488bcf                  mov      rcx, rdi
0000000140cfed32  884331                  mov      byte ptr [rbx + 0x31], al
0000000140cfed35  e88693d701              call     0x142a780c0
0000000140cfed3a  488bcf                  mov      rcx, rdi
0000000140cfed3d  0fb6e8                  movzx    ebp, al
0000000140cfed40  e8db94d701              call     0x142a78220
0000000140cfed45  488bcf                  mov      rcx, rdi
0000000140cfed48  66894332                mov      word ptr [rbx + 0x32], ax
0000000140cfed4c  e81f95d701              call     0x142a78270
0000000140cfed51  488bcf                  mov      rcx, rdi
0000000140cfed54  894340                  mov      dword ptr [rbx + 0x40], eax
0000000140cfed57  e89493d701              call     0x142a780f0
0000000140cfed5c  488bcf                  mov      rcx, rdi
0000000140cfed5f  f30f114350              movss    dword ptr [rbx + 0x50], xmm0
0000000140cfed64  e88793d701              call     0x142a780f0
0000000140cfed69  4c8bc6                  mov      r8, rsi
0000000140cfed6c  f30f114354              movss    dword ptr [rbx + 0x54], xmm0
0000000140cfed71  488d5334                lea      rdx, [rbx + 0x34]
0000000140cfed75  488bcf                  mov      rcx, rdi
0000000140cfed78  e89319ffff              call     0x140cf0710
0000000140cfed7d  837b3c00                cmp      dword ptr [rbx + 0x3c], 0
0000000140cfed81  7c09                    jl       0x140cfed8c
0000000140cfed83  c7434400000000          mov      dword ptr [rbx + 0x44], 0
0000000140cfed8a  eb2d                    jmp      0x140cfedb9
0000000140cfed8c  8b4b34                  mov      ecx, dword ptr [rbx + 0x34]
0000000140cfed8f  8d41ea                  lea      eax, [rcx - 0x16]
0000000140cfed92  3dfb010000              cmp      eax, 0x1fb
0000000140cfed97  7709                    ja       0x140cfeda2
0000000140cfed99  c7434401000000          mov      dword ptr [rbx + 0x44], 1
0000000140cfeda0  eb17                    jmp      0x140cfedb9
0000000140cfeda2  8d81ecfdffff            lea      eax, [rcx - 0x214]
0000000140cfeda8  83f87e                  cmp      eax, 0x7e
0000000140cfedab  7705                    ja       0x140cfedb2
0000000140cfedad  896b44                  mov      dword ptr [rbx + 0x44], ebp
0000000140cfedb0  eb07                    jmp      0x140cfedb9
0000000140cfedb2  c74344ffffffff          mov      dword ptr [rbx + 0x44], 0xffffffff
0000000140cfedb9  488b6c2450              mov      rbp, qword ptr [rsp + 0x50]
0000000140cfedbe  4c8b742458              mov      r14, qword ptr [rsp + 0x58]
0000000140cfedc3  488b5c2460              mov      rbx, qword ptr [rsp + 0x60]
0000000140cfedc8  488b742468              mov      rsi, qword ptr [rsp + 0x68]
0000000140cfedcd  4883c440                add      rsp, 0x40
0000000140cfedd1  5f                      pop      rdi
0000000140cfedd2  c3                      ret

# automation_writer: 0x140d0f570 bytes=190 sha256=5bc163302b2024f9c8ea6a9c14b548ecc2993938cc6a71e80ad9efa15553e520
0000000140d0f570  48895c2408              mov      qword ptr [rsp + 8], rbx
0000000140d0f575  57                      push     rdi
0000000140d0f576  4883ec40                sub      rsp, 0x40
0000000140d0f57a  448b4128                mov      r8d, dword ptr [rcx + 0x28]
0000000140d0f57e  488bfa                  mov      rdi, rdx
0000000140d0f581  488bd9                  mov      rbx, rcx
0000000140d0f584  4585c0                  test     r8d, r8d
0000000140d0f587  741a                    je       0x140d0f5a3
0000000140d0f589  4183e801                sub      r8d, 1
0000000140d0f58d  740d                    je       0x140d0f59c
0000000140d0f58f  4183f801                cmp      r8d, 1
0000000140d0f593  757d                    jne      0x140d0f612
0000000140d0f595  ba02000000              mov      edx, 2
0000000140d0f59a  eb09                    jmp      0x140d0f5a5
0000000140d0f59c  ba01000000              mov      edx, 1
0000000140d0f5a1  eb02                    jmp      0x140d0f5a5
0000000140d0f5a3  33d2                    xor      edx, edx
0000000140d0f5a5  488bcf                  mov      rcx, rdi
0000000140d0f5a8  e803e8d601              call     0x142a7ddb0
0000000140d0f5ad  0fb65331                movzx    edx, byte ptr [rbx + 0x31]
0000000140d0f5b1  488bcf                  mov      rcx, rdi
0000000140d0f5b4  e8e7e5d601              call     0x142a7dba0
0000000140d0f5b9  0fb75332                movzx    edx, word ptr [rbx + 0x32]
0000000140d0f5bd  488bcf                  mov      rcx, rdi
0000000140d0f5c0  e8abe7d601              call     0x142a7dd70
0000000140d0f5c5  8b5340                  mov      edx, dword ptr [rbx + 0x40]
0000000140d0f5c8  488bcf                  mov      rcx, rdi
0000000140d0f5cb  e8e0e7d601              call     0x142a7ddb0
0000000140d0f5d0  8b5344                  mov      edx, dword ptr [rbx + 0x44]
0000000140d0f5d3  488bcf                  mov      rcx, rdi
0000000140d0f5d6  e8d5e7d601              call     0x142a7ddb0
0000000140d0f5db  f30f104b50              movss    xmm1, dword ptr [rbx + 0x50]
0000000140d0f5e0  488bcf                  mov      rcx, rdi
0000000140d0f5e3  e8e8e6d601              call     0x142a7dcd0
0000000140d0f5e8  f30f104b54              movss    xmm1, dword ptr [rbx + 0x54]
0000000140d0f5ed  488bcf                  mov      rcx, rdi
0000000140d0f5f0  e8dbe6d601              call     0x142a7dcd0
0000000140d0f5f5  8b4b34                  mov      ecx, dword ptr [rbx + 0x34]
0000000140d0f5f8  e80364beff              call     0x1408f5a00
0000000140d0f5fd  488bd0                  mov      rdx, rax
0000000140d0f600  488bcf                  mov      rcx, rdi
0000000140d0f603  488b5c2450              mov      rbx, qword ptr [rsp + 0x50]
0000000140d0f608  4883c440                add      rsp, 0x40
0000000140d0f60c  5f                      pop      rdi
0000000140d0f60d  e9cee8d601              jmp      0x142a7dee0
0000000140d0f612  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140d0f617  e8a47ed7ff              call     0x140a874c0
0000000140d0f61c  488d156d7f1209          lea      rdx, [rip + 0x9127f6d]
0000000140d0f623  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140d0f628  e8a9196c03              call     0x1443d0fd6
0000000140d0f62d  cc                      int3

# automation_mode_reader: 0x140cf0360 bytes=96 sha256=b9dee7e900413b264b3dbd241b835fda1cc111b22a2b05de207154e76f26f341
0000000140cf0360  4053                    push     rbx
0000000140cf0362  4883ec40                sub      rsp, 0x40
0000000140cf0366  488bda                  mov      rbx, rdx
0000000140cf0369  e8027fd801              call     0x142a78270
0000000140cf036e  85c0                    test     eax, eax
0000000140cf0370  7422                    je       0x140cf0394
0000000140cf0372  83e801                  sub      eax, 1
0000000140cf0375  7411                    je       0x140cf0388
0000000140cf0377  83f801                  cmp      eax, 1
0000000140cf037a  7524                    jne      0x140cf03a0
0000000140cf037c  c70302000000            mov      dword ptr [rbx], 2
0000000140cf0382  4883c440                add      rsp, 0x40
0000000140cf0386  5b                      pop      rbx
0000000140cf0387  c3                      ret
0000000140cf0388  c70301000000            mov      dword ptr [rbx], 1
0000000140cf038e  4883c440                add      rsp, 0x40
0000000140cf0392  5b                      pop      rbx
0000000140cf0393  c3                      ret
0000000140cf0394  c70300000000            mov      dword ptr [rbx], 0
0000000140cf039a  4883c440                add      rsp, 0x40
0000000140cf039e  5b                      pop      rbx
0000000140cf039f  c3                      ret
0000000140cf03a0  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140cf03a5  e81671d9ff              call     0x140a874c0
0000000140cf03aa  488d15df711409          lea      rdx, [rip + 0x91471df]
0000000140cf03b1  488d4c2420              lea      rcx, [rsp + 0x20]
0000000140cf03b6  e81b0c6e03              call     0x1443d0fd6
0000000140cf03bb  cc                      int3
0000000140cf03bc  cc                      int3
0000000140cf03bd  cc                      int3
0000000140cf03be  cc                      int3
0000000140cf03bf  cc                      int3

# automation_tag_reader: 0x140cf0710 bytes=171 sha256=c2912e28fef4fa8996ef438e437a3c7855c609f5cdfa36b4a19df59e69ed7076
0000000140cf0710  4053                    push     rbx
0000000140cf0712  4883ec50                sub      rsp, 0x50
0000000140cf0716  488b05a3515b09          mov      rax, qword ptr [rip + 0x95b51a3]
0000000140cf071d  4833c4                  xor      rax, rsp
0000000140cf0720  4889442440              mov      qword ptr [rsp + 0x40], rax
0000000140cf0725  488bda                  mov      rbx, rdx
0000000140cf0728  488d542420              lea      rdx, [rsp + 0x20]
0000000140cf072d  e8fe7bd801              call     0x142a78330
0000000140cf0732  90                      nop
0000000140cf0733  488378180f              cmp      qword ptr [rax + 0x18], 0xf
0000000140cf0738  7603                    jbe      0x140cf073d
0000000140cf073a  488b00                  mov      rax, qword ptr [rax]
0000000140cf073d  488bc8                  mov      rcx, rax
0000000140cf0740  e8dba7c7ff              call     0x14096af20
0000000140cf0745  8903                    mov      dword ptr [rbx], eax
0000000140cf0747  488b542438              mov      rdx, qword ptr [rsp + 0x38]
0000000140cf074c  4883fa0f                cmp      rdx, 0xf
0000000140cf0750  7635                    jbe      0x140cf0787
0000000140cf0752  48ffc2                  inc      rdx
0000000140cf0755  488b4c2420              mov      rcx, qword ptr [rsp + 0x20]
0000000140cf075a  488bc1                  mov      rax, rcx
0000000140cf075d  4881fa00100000          cmp      rdx, 0x1000
0000000140cf0764  721c                    jb       0x140cf0782
0000000140cf0766  4883c227                add      rdx, 0x27
0000000140cf076a  488b49f8                mov      rcx, qword ptr [rcx - 8]
0000000140cf076e  482bc1                  sub      rax, rcx
0000000140cf0771  4883c0f8                add      rax, -8
0000000140cf0775  4883f81f                cmp      rax, 0x1f
0000000140cf0779  7607                    jbe      0x140cf0782
0000000140cf077b  ff151f09a003            call     qword ptr [rip + 0x3a0091f]
0000000140cf0781  cc                      int3
0000000140cf0782  e8498a6d03              call     0x1443c91d0
0000000140cf0787  488b4c2440              mov      rcx, qword ptr [rsp + 0x40]
0000000140cf078c  4833cc                  xor      rcx, rsp
0000000140cf078f  e8fc8d6d03              call     0x1443c9590
0000000140cf0794  4883c450                add      rsp, 0x50
0000000140cf0798  5b                      pop      rbx
0000000140cf0799  c3                      ret
0000000140cf079a  cc                      int3
0000000140cf079b  cc                      int3
0000000140cf079c  cc                      int3
0000000140cf079d  cc                      int3
0000000140cf079e  cc                      int3
0000000140cf079f  cc                      int3
0000000140cf07a0  4053                    push     rbx
0000000140cf07a2  4883ec50                sub      rsp, 0x50
0000000140cf07a6  488b0513515b09          mov      rax, qword ptr [rip + 0x95b5113]
0000000140cf07ad  4833c4                  xor      rax, rsp
0000000140cf07b0  4889442448              mov      qword ptr [rsp + 0x48], rax
0000000140cf07b5  488bda                  mov      rbx, rdx

# automation_sertype: 0x14051b380 bytes=6 sha256=2db31f4e09597946e56e859813bfdad7046d457c32332c3a882654d1e3ffdf67
000000014051b380  b801000000              mov      eax, 1
000000014051b385  c3                      ret

# automation_version: 0x1406b10f0 bytes=6 sha256=a58a135412db7935e8c860ea407de5ebd382aa1a29ecef8615d3853d02013691
00000001406b10f0  b871000000              mov      eax, 0x71
00000001406b10f5  c3                      ret

; automation_text_reader
000000014091f1b0  4c8bdc                           mov r11, rsp
000000014091f1b3  56                               push rsi
000000014091f1b4  4154                             push r12
000000014091f1b6  4156                             push r14
000000014091f1b8  4157                             push r15
000000014091f1ba  4881ecd8000000                   sub rsp, 0xd8
000000014091f1c1  488b05f8669809                   mov rax, qword ptr [rip + 0x99866f8]
000000014091f1c8  4833c4                           xor rax, rsp
000000014091f1cb  48898424c0000000                 mov qword ptr [rsp + 0xc0], rax
000000014091f1d3  49895b10                         mov qword ptr [r11 + 0x10], rbx
000000014091f1d7  4533e4                           xor r12d, r12d
000000014091f1da  498b5908                         mov rbx, qword ptr [r9 + 8]
000000014091f1de  4032f6                           xor sil, sil
000000014091f1e1  4d896bd8                         mov qword ptr [r11 - 0x28], r13
000000014091f1e5  4c8bf9                           mov r15, rcx
000000014091f1e8  4d8b6910                         mov r13, qword ptr [r9 + 0x10]
000000014091f1ec  458bf4                           mov r14d, r12d
000000014091f1ef  448944242c                       mov dword ptr [rsp + 0x2c], r8d
000000014091f1f4  895138                           mov dword ptr [rcx + 0x38], edx
000000014091f1f7  4489413c                         mov dword ptr [rcx + 0x3c], r8d
000000014091f1fb  4088742420                       mov byte ptr [rsp + 0x20], sil
000000014091f200  493bdd                           cmp rbx, r13
000000014091f203  0f84a2030000                     je 0x14091f5ab
000000014091f209  49896b18                         mov qword ptr [r11 + 0x18], rbp
000000014091f20d  4c8d05ec035f04                   lea r8, [rip + 0x45f03ec]
000000014091f214  49897b20                         mov qword ptr [r11 + 0x20], rdi
000000014091f218  0f1f840000000000                 nop dword ptr [rax + rax]
000000014091f220  488b13                           mov rdx, qword ptr [rbx]
000000014091f223  488bfb                           mov rdi, rbx
000000014091f226  498bcc                           mov rcx, r12
000000014091f229  0f1f8000000000                   nop dword ptr [rax]
000000014091f230  0fb6040a                         movzx eax, byte ptr [rdx + rcx]
000000014091f234  48ffc1                           inc rcx
000000014091f237  413a4408ff                       cmp al, byte ptr [r8 + rcx - 1]
000000014091f23c  0f85c2000000                     jne 0x14091f304
000000014091f242  4883f908                         cmp rcx, 8
000000014091f246  75e8                             jne 0x14091f230
000000014091f248  488b4b08                         mov rcx, qword ptr [rbx + 8]
000000014091f24c  488d542440                       lea rdx, [rsp + 0x40]
000000014091f251  41ffc6                           inc r14d
000000014091f254  482bd1                           sub rdx, rcx
000000014091f257  660f1f840000000000               nop word ptr [rax + rax]
000000014091f260  0fb601                           movzx eax, byte ptr [rcx]
000000014091f263  88040a                           mov byte ptr [rdx + rcx], al
000000014091f266  488d4901                         lea rcx, [rcx + 1]
000000014091f26a  84c0                             test al, al
000000014091f26c  75f2                             jne 0x14091f260
000000014091f26e  4883c310                         add rbx, 0x10
000000014091f272  493bdd                           cmp rbx, r13
000000014091f275  744b                             je 0x14091f2c2
000000014091f277  488b13                           mov rdx, qword ptr [rbx]
000000014091f27a  498bcc                           mov rcx, r12
000000014091f27d  0f1f00                           nop dword ptr [rax]
000000014091f280  0fb6040a                         movzx eax, byte ptr [rdx + rcx]
000000014091f284  48ffc1                           inc rcx
000000014091f287  413a4408ff                       cmp al, byte ptr [r8 + rcx - 1]
000000014091f28c  7534                             jne 0x14091f2c2
000000014091f28e  4883f908                         cmp rcx, 8
000000014091f292  75ec                             jne 0x14091f280
000000014091f294  488b4b08                         mov rcx, qword ptr [rbx + 8]
000000014091f298  488d542440                       lea rdx, [rsp + 0x40]
000000014091f29d  41ffc6                           inc r14d
000000014091f2a0  482bd1                           sub rdx, rcx
000000014091f2a3  0f1f4000                         nop dword ptr [rax]
000000014091f2a7  660f1f840000000000               nop word ptr [rax + rax]
000000014091f2b0  0fb601                           movzx eax, byte ptr [rcx]
000000014091f2b3  88040a                           mov byte ptr [rdx + rcx], al
000000014091f2b6  488d4901                         lea rcx, [rcx + 1]
000000014091f2ba  84c0                             test al, al
000000014091f2bc  75f2                             jne 0x14091f2b0
000000014091f2be  4883c310                         add rbx, 0x10
000000014091f2c2  4c8d4c2428                       lea r9, [rsp + 0x28]
000000014091f2c7  4c8d442424                       lea r8, [rsp + 0x24]
000000014091f2cc  488d156d6e5504                   lea rdx, [rip + 0x4556e6d]
000000014091f2d3  488d4c2440                       lea rcx, [rsp + 0x40]
000000014091f2d8  e8f30be5ff                       call 0x14076fed0
000000014091f2dd  83f802                           cmp eax, 2
000000014091f2e0  7522                             jne 0x14091f304
000000014091f2e2  0fb7542424                       movzx edx, word ptr [rsp + 0x24]
000000014091f2e7  440fb7442428                     movzx r8d, word ptr [rsp + 0x28]
000000014091f2ed  8bca                             mov ecx, edx
000000014091f2ef  c1e108                           shl ecx, 8
000000014091f2f2  410bc8                           or ecx, r8d
000000014091f2f5  740d                             je 0x14091f304
000000014091f2f7  66c1e208                         shl dx, 8
000000014091f2fb  66410bd0                         or dx, r8w
000000014091f2ff  664189575e                       mov word ptr [r15 + 0x5e], dx
000000014091f304  493bdd                           cmp rbx, r13
000000014091f307  0f8466020000                     je 0x14091f573
000000014091f30d  488b2b                           mov rbp, qword ptr [rbx]
000000014091f310  488d15f1025f04                   lea rdx, [rip + 0x45f02f1]
000000014091f317  488bcd                           mov rcx, rbp
000000014091f31a  e8a71dab03                       call 0x1443d10c6
000000014091f31f  85c0                             test eax, eax
000000014091f321  7578                             jne 0x14091f39b
000000014091f323  488b7308                         mov rsi, qword ptr [rbx + 8]
000000014091f327  488d1582905504                   lea rdx, [rip + 0x4559082]
000000014091f32e  498bcc                           mov rcx, r12
000000014091f331  0fb6040e                         movzx eax, byte ptr [rsi + rcx]
000000014091f335  48ffc1                           inc rcx
000000014091f338  3a440aff                         cmp al, byte ptr [rdx + rcx - 1]
000000014091f33c  7512                             jne 0x14091f350
000000014091f33e  4883f908                         cmp rcx, 8
000000014091f342  75ed                             jne 0x14091f331
000000014091f344  41ffc6                           inc r14d
000000014091f347  45896728                         mov dword ptr [r15 + 0x28], r12d
000000014091f34b  e92d020000                       jmp 0x14091f57d
000000014091f350  488d15c1025f04                   lea rdx, [rip + 0x45f02c1]
000000014091f357  488bce                           mov rcx, rsi
000000014091f35a  e8671dab03                       call 0x1443d10c6
000000014091f35f  85c0                             test eax, eax
000000014091f361  7510                             jne 0x14091f373
000000014091f363  41ffc6                           inc r14d
000000014091f366  41c7472801000000                 mov dword ptr [r15 + 0x28], 1
000000014091f36e  e90a020000                       jmp 0x14091f57d
000000014091f373  488d15ae025f04                   lea rdx, [rip + 0x45f02ae]
000000014091f37a  488bce                           mov rcx, rsi
000000014091f37d  e8441dab03                       call 0x1443d10c6
000000014091f382  85c0                             test eax, eax
000000014091f384  7510                             jne 0x14091f396
000000014091f386  41ffc6                           inc r14d
000000014091f389  41c7472802000000                 mov dword ptr [r15 + 0x28], 2
000000014091f391  e9e7010000                       jmp 0x14091f57d
000000014091f396  0fb6742420                       movzx esi, byte ptr [rsp + 0x20]
000000014091f39b  488d1596025f04                   lea rdx, [rip + 0x45f0296]
000000014091f3a2  488bcd                           mov rcx, rbp
000000014091f3a5  e81c1dab03                       call 0x1443d10c6
000000014091f3aa  488d2d97025f04                   lea rbp, [rip + 0x45f0297]
000000014091f3b1  85c0                             test eax, eax
000000014091f3b3  752d                             jne 0x14091f3e2
000000014091f3b5  488b5308                         mov rdx, qword ptr [rbx + 8]
000000014091f3b9  41ffc6                           inc r14d
000000014091f3bc  498bc4                           mov rax, r12
000000014091f3bf  90                               nop
000000014091f3c0  0fb60c02                         movzx ecx, byte ptr [rdx + rax]
000000014091f3c4  48ffc0                           inc rax
000000014091f3c7  3a4c28ff                         cmp cl, byte ptr [rax + rbp - 1]
000000014091f3cb  750a                             jne 0x14091f3d7
000000014091f3cd  4883f804                         cmp rax, 4
000000014091f3d1  75ed                             jne 0x14091f3c0
000000014091f3d3  3a4c28ff                         cmp cl, byte ptr [rax + rbp - 1]
000000014091f3d7  0f94c0                           sete al
000000014091f3da  4883c310                         add rbx, 0x10
000000014091f3de  41884731                         mov byte ptr [r15 + 0x31], al
000000014091f3e2  493bdd                           cmp rbx, r13
000000014091f3e5  0f8488010000                     je 0x14091f573
000000014091f3eb  488b0b                           mov rcx, qword ptr [rbx]
000000014091f3ee  488d155b025f04                   lea rdx, [rip + 0x45f025b]
000000014091f3f5  e8cc1cab03                       call 0x1443d10c6
000000014091f3fa  85c0                             test eax, eax
000000014091f3fc  753f                             jne 0x14091f43d
000000014091f3fe  488b5308                         mov rdx, qword ptr [rbx + 8]
000000014091f402  41ffc6                           inc r14d
000000014091f405  498bc4                           mov rax, r12
000000014091f408  0f1f840000000000                 nop dword ptr [rax + rax]
000000014091f410  0fb60c02                         movzx ecx, byte ptr [rdx + rax]
000000014091f414  48ffc0                           inc rax
000000014091f417  3a4c28ff                         cmp cl, byte ptr [rax + rbp - 1]
000000014091f41b  750a                             jne 0x14091f427
000000014091f41d  4883f804                         cmp rax, 4
000000014091f421  75ed                             jne 0x14091f410
000000014091f423  3a4c28ff                         cmp cl, byte ptr [rax + rbp - 1]
000000014091f427  400f94c6                         sete sil
000000014091f42b  4883c310                         add rbx, 0x10
000000014091f42f  4088742420                       mov byte ptr [rsp + 0x20], sil
000000014091f434  493bdd                           cmp rbx, r13
000000014091f437  0f8436010000                     je 0x14091f573
000000014091f43d  488b13                           mov rdx, qword ptr [rbx]
000000014091f440  4c8d0529025f04                   lea r8, [rip + 0x45f0229]
000000014091f447  498bcc                           mov rcx, r12
000000014091f44a  660f1f440000                     nop word ptr [rax + rax]
000000014091f450  0fb6040a                         movzx eax, byte ptr [rdx + rcx]
000000014091f454  48ffc1                           inc rcx
000000014091f457  413a4408ff                       cmp al, byte ptr [r8 + rcx - 1]
000000014091f45c  7525                             jne 0x14091f483
000000014091f45e  4883f907                         cmp rcx, 7
000000014091f462  75ec                             jne 0x14091f450
000000014091f464  488b4b08                         mov rcx, qword ptr [rbx + 8]
000000014091f468  41ffc6                           inc r14d
000000014091f46b  ff151f18dd03                     call qword ptr [rip + 0x3dd181f]
000000014091f471  4883c310                         add rbx, 0x10
000000014091f475  6641894732                       mov word ptr [r15 + 0x32], ax
000000014091f47a  493bdd                           cmp rbx, r13
000000014091f47d  0f84f0000000                     je 0x14091f573
000000014091f483  488b13                           mov rdx, qword ptr [rbx]
000000014091f486  4c8d05eb015f04                   lea r8, [rip + 0x45f01eb]
000000014091f48d  498bcc                           mov rcx, r12
000000014091f490  0fb6040a                         movzx eax, byte ptr [rdx + rcx]
000000014091f494  48ffc1                           inc rcx
000000014091f497  413a4408ff                       cmp al, byte ptr [r8 + rcx - 1]
000000014091f49c  751a                             jne 0x14091f4b8
000000014091f49e  4883f907                         cmp rcx, 7
000000014091f4a2  75ec                             jne 0x14091f490
000000014091f4a4  488b4b08                         mov rcx, qword ptr [rbx + 8]
000000014091f4a8  e873ba0400                       call 0x14096af20
000000014091f4ad  41ffc6                           inc r14d
000000014091f4b0  41894734                         mov dword ptr [r15 + 0x34], eax
000000014091f4b4  4883c310                         add rbx, 0x10
000000014091f4b8  493bdd                           cmp rbx, r13
000000014091f4bb  0f84b2000000                     je 0x14091f573
000000014091f4c1  488b13                           mov rdx, qword ptr [rbx]
000000014091f4c4  4c8d05b5015f04                   lea r8, [rip + 0x45f01b5]
000000014091f4cb  498bcc                           mov rcx, r12
000000014091f4ce  6690                             nop
000000014091f4d0  0fb6040a                         movzx eax, byte ptr [rdx + rcx]
000000014091f4d4  48ffc1                           inc rcx
000000014091f4d7  413a4408ff                       cmp al, byte ptr [r8 + rcx - 1]
000000014091f4dc  7520                             jne 0x14091f4fe
000000014091f4de  4883f907                         cmp rcx, 7
000000014091f4e2  75ec                             jne 0x14091f4d0
000000014091f4e4  488b4b08                         mov rcx, qword ptr [rbx + 8]
000000014091f4e8  41ffc6                           inc r14d
000000014091f4eb  ff159f17dd03                     call qword ptr [rip + 0x3dd179f]
000000014091f4f1  4883c310                         add rbx, 0x10
000000014091f4f5  41894740                         mov dword ptr [r15 + 0x40], eax
000000014091f4f9  493bdd                           cmp rbx, r13
000000014091f4fc  7475                             je 0x14091f573
000000014091f4fe  488b0b                           mov rcx, qword ptr [rbx]
000000014091f501  488d1580015f04                   lea rdx, [rip + 0x45f0180]
000000014091f508  e8b91bab03                       call 0x1443d10c6
000000014091f50d  85c0                             test eax, eax
000000014091f50f  751e                             jne 0x14091f52f
000000014091f511  488b4b08                         mov rcx, qword ptr [rbx + 8]
000000014091f515  41ffc6                           inc r14d
000000014091f518  ff153217dd03                     call qword ptr [rip + 0x3dd1732]
000000014091f51e  0f57c9                           xorps xmm1, xmm1
000000014091f521  4883c310                         add rbx, 0x10
000000014091f525  f20f5ac8                         cvtsd2ss xmm1, xmm0
000000014091f529  f3410f114f50                     movss dword ptr [r15 + 0x50], xmm1
000000014091f52f  493bdd                           cmp rbx, r13
000000014091f532  743f                             je 0x14091f573
000000014091f534  488b13                           mov rdx, qword ptr [rbx]
000000014091f537  4c8d055a015f04                   lea r8, [rip + 0x45f015a]
000000014091f53e  498bcc                           mov rcx, r12
000000014091f541  0fb6040a                         movzx eax, byte ptr [rdx + rcx]
000000014091f545  48ffc1                           inc rcx
000000014091f548  413a4408ff                       cmp al, byte ptr [r8 + rcx - 1]
000000014091f54d  7524                             jne 0x14091f573
000000014091f54f  4883f908                         cmp rcx, 8
000000014091f553  75ec                             jne 0x14091f541
000000014091f555  488b4b08                         mov rcx, qword ptr [rbx + 8]
000000014091f559  41ffc6                           inc r14d
000000014091f55c  ff15ee16dd03                     call qword ptr [rip + 0x3dd16ee]
000000014091f562  0f57c9                           xorps xmm1, xmm1
000000014091f565  4883c310                         add rbx, 0x10
000000014091f569  f20f5ac8                         cvtsd2ss xmm1, xmm0
000000014091f56d  f3410f114f54                     movss dword ptr [r15 + 0x54], xmm1
000000014091f573  483bdf                           cmp rbx, rdi
000000014091f576  7509                             jne 0x14091f581
000000014091f578  4088742420                       mov byte ptr [rsp + 0x20], sil
000000014091f57d  4883c310                         add rbx, 0x10
000000014091f581  0fb6742420                       movzx esi, byte ptr [rsp + 0x20]
000000014091f586  4c8d0573005f04                   lea r8, [rip + 0x45f0073]
000000014091f58d  493bdd                           cmp rbx, r13
000000014091f590  0f858afcffff                     jne 0x14091f220
000000014091f596  488bbc2418010000                 mov rdi, qword ptr [rsp + 0x118]
000000014091f59e  488bac2410010000                 mov rbp, qword ptr [rsp + 0x110]
000000014091f5a6  448b44242c                       mov r8d, dword ptr [rsp + 0x2c]
000000014091f5ab  6641837f5e70                     cmp word ptr [r15 + 0x5e], 0x70
000000014091f5b1  4c8bac24d0000000                 mov r13, qword ptr [rsp + 0xd0]
000000014091f5b9  488b9c2408010000                 mov rbx, qword ptr [rsp + 0x108]
000000014091f5c1  737f                             jae 0x14091f642
000000014091f5c3  41817f3496020000                 cmp dword ptr [r15 + 0x34], 0x296
000000014091f5cb  7575                             jne 0x14091f642
000000014091f5cd  f3410f105750                     movss xmm2, dword ptr [r15 + 0x50]
000000014091f5d3  f30f100df5c4de03                 movss xmm1, dword ptr [rip + 0x3dec4f5]
000000014091f5db  0f2fd1                           comiss xmm2, xmm1
000000014091f5de  f30f101d220be403                 movss xmm3, dword ptr [rip + 0x3e40b22]
000000014091f5e6  720e                             jb 0x14091f5f6
000000014091f5e8  f30f5cd1                         subss xmm2, xmm1
000000014091f5ec  f30f59d3                         mulss xmm2, xmm3
000000014091f5f0  f30f58d1                         addss xmm2, xmm1
000000014091f5f4  eb12                             jmp 0x14091f608
000000014091f5f6  0f28c1                           movaps xmm0, xmm1
000000014091f5f9  f30f5cc2                         subss xmm0, xmm2
000000014091f5fd  0f28d1                           movaps xmm2, xmm1
000000014091f600  f30f59c3                         mulss xmm0, xmm3
000000014091f604  f30f5cd0                         subss xmm2, xmm0
000000014091f608  f3410f115750                     movss dword ptr [r15 + 0x50], xmm2
000000014091f60e  f3410f105754                     movss xmm2, dword ptr [r15 + 0x54]
000000014091f614  0f2fd1                           comiss xmm2, xmm1
000000014091f617  7214                             jb 0x14091f62d
000000014091f619  f30f5cd1                         subss xmm2, xmm1
000000014091f61d  f30f59d3                         mulss xmm2, xmm3
000000014091f621  f30f58d1                         addss xmm2, xmm1
000000014091f625  f3410f115754                     movss dword ptr [r15 + 0x54], xmm2
000000014091f62b  eb15                             jmp 0x14091f642
000000014091f62d  0f28c1                           movaps xmm0, xmm1
000000014091f630  f30f5cc2                         subss xmm0, xmm2
000000014091f634  f30f59c3                         mulss xmm0, xmm3
000000014091f638  f30f5cc8                         subss xmm1, xmm0
000000014091f63c  f3410f114f54                     movss dword ptr [r15 + 0x54], xmm1
000000014091f642  4585c0                           test r8d, r8d
000000014091f645  792a                             jns 0x14091f671
000000014091f647  418b4f34                         mov ecx, dword ptr [r15 + 0x34]
000000014091f64b  8d41ea                           lea eax, [rcx - 0x16]
000000014091f64e  3dfb010000                       cmp eax, 0x1fb
000000014091f653  770a                             ja 0x14091f65f
000000014091f655  41c7474401000000                 mov dword ptr [r15 + 0x44], 1
000000014091f65d  eb20                             jmp 0x14091f67f
000000014091f65f  8d81ecfdffff                     lea eax, [rcx - 0x214]
000000014091f665  83f87e                           cmp eax, 0x7e
000000014091f668  770d                             ja 0x14091f677
000000014091f66a  4084f6                           test sil, sil
000000014091f66d  410f95c4                         setne r12b
000000014091f671  45896744                         mov dword ptr [r15 + 0x44], r12d
000000014091f675  eb08                             jmp 0x14091f67f
000000014091f677  41c74744ffffffff                 mov dword ptr [r15 + 0x44], 0xffffffff
000000014091f67f  418bc6                           mov eax, r14d
000000014091f682  488b8c24c0000000                 mov rcx, qword ptr [rsp + 0xc0]
000000014091f68a  4833cc                           xor rcx, rsp
000000014091f68d  e8fe9eaa03                       call 0x1443c9590
000000014091f692  4881c4d8000000                   add rsp, 0xd8
000000014091f699  415f                             pop r15
000000014091f69b  415e                             pop r14
000000014091f69d  415c                             pop r12
000000014091f69f  5e                               pop rsi
000000014091f6a0  c3                               ret
000000014091f6a1  cc                               int3
000000014091f6a2  cc                               int3
000000014091f6a3  cc                               int3
000000014091f6a4  cc                               int3
000000014091f6a5  cc                               int3
000000014091f6a6  cc                               int3
000000014091f6a7  cc                               int3
000000014091f6a8  cc                               int3
000000014091f6a9  cc                               int3
000000014091f6aa  cc                               int3
000000014091f6ab  cc                               int3
000000014091f6ac  cc                               int3
000000014091f6ad  cc                               int3
000000014091f6ae  cc                               int3
000000014091f6af  cc                               int3
000000014091f6b0  48895c2410                       mov qword ptr [rsp + 0x10], rbx
000000014091f6b5  4889742418                       mov qword ptr [rsp + 0x18], rsi
000000014091f6ba  48897c2420                       mov qword ptr [rsp + 0x20], rdi
000000014091f6bf  55                               push rbp
000000014091f6c0  4154                             push r12
000000014091f6c2  4155                             push r13
000000014091f6c4  4156                             push r14
000000014091f6c6  4157                             push r15
000000014091f6c8  488d6c24d0                       lea rbp, [rsp - 0x30]
000000014091f6cd  4881ec30010000                   sub rsp, 0x130
000000014091f6d4  488b05e5619809                   mov rax, qword ptr [rip + 0x99861e5]
000000014091f6db  4833c4                           xor rax, rsp
000000014091f6de  48894520                         mov qword ptr [rbp + 0x20], rax
000000014091f6e2  488bd9                           mov rbx, rcx
000000014091f6e5  48894c2450                       mov qword ptr [rsp + 0x50], rcx

; automation_runtime_index
000000014096e6b0  48894c2408                       mov qword ptr [rsp + 8], rcx
000000014096e6b5  53                               push rbx
000000014096e6b6  55                               push rbp
000000014096e6b7  56                               push rsi
000000014096e6b8  57                               push rdi
000000014096e6b9  4154                             push r12
000000014096e6bb  4155                             push r13
000000014096e6bd  4156                             push r14
000000014096e6bf  4157                             push r15
000000014096e6c1  4883ec38                         sub rsp, 0x38
000000014096e6c5  85d2                             test edx, edx
000000014096e6c7  8d4201                           lea eax, [rdx + 1]
000000014096e6ca  488bf9                           mov rdi, rcx
000000014096e6cd  b940000000                       mov ecx, 0x40
000000014096e6d2  0f48c1                           cmovs eax, ecx
000000014096e6d5  4533ff                           xor r15d, r15d
000000014096e6d8  85d2                             test edx, edx
000000014096e6da  458bf7                           mov r14d, r15d
000000014096e6dd  440f49f2                         cmovns r14d, edx
000000014096e6e1  443bf0                           cmp r14d, eax
000000014096e6e4  0f8d22050000                     jge 0x14096ec0c
000000014096e6ea  4963ee                           movsxd rbp, r14d
000000014096e6ed  4898                             cdqe
000000014096e6ef  41c1e607                         shl r14d, 7
000000014096e6f3  4889ac2490000000                 mov qword ptr [rsp + 0x90], rbp
000000014096e6fb  488d0ced18000000                 lea rcx, [rbp*8 + 0x18]
000000014096e703  4889442420                       mov qword ptr [rsp + 0x20], rax
000000014096e708  48898c2498000000                 mov qword ptr [rsp + 0x98], rcx
000000014096e710  4489b42488000000                 mov dword ptr [rsp + 0x88], r14d
000000014096e718  0f1f840000000000                 nop dword ptr [rax + rax]
000000014096e720  488b87f8d70200                   mov rax, qword ptr [rdi + 0x2d7f8]
000000014096e727  488b3401                         mov rsi, qword ptr [rcx + rax]
000000014096e72b  488d8ea8040000                   lea rcx, [rsi + 0x4a8]
000000014096e732  e8988fa503                       call 0x1443c76cf
000000014096e737  85c0                             test eax, eax
000000014096e739  0f8522050000                     jne 0x14096ec61
000000014096e73f  8b86f4040000                     mov eax, dword ptr [rsi + 0x4f4]
000000014096e745  3dffffff7f                       cmp eax, 0x7fffffff
000000014096e74a  0f84fe040000                     je 0x14096ec4e
000000014096e750  488d8e20050000                   lea rcx, [rsi + 0x520]
000000014096e757  ba08000000                       mov edx, 8
000000014096e75c  0f1f4000                         nop dword ptr [rax]
000000014096e760  488d41e0                         lea rax, [rcx - 0x20]
000000014096e764  488941e8                         mov qword ptr [rcx - 0x18], rax
000000014096e768  488900                           mov qword ptr [rax], rax
000000014096e76b  488d41f8                         lea rax, [rcx - 8]
000000014096e76f  4c8979d8                         mov qword ptr [rcx - 0x28], r15
000000014096e773  488900                           mov qword ptr [rax], rax
000000014096e776  488901                           mov qword ptr [rcx], rax
000000014096e779  488d4110                         lea rax, [rcx + 0x10]
000000014096e77d  4c8979f0                         mov qword ptr [rcx - 0x10], r15
000000014096e781  488900                           mov qword ptr [rax], rax
000000014096e784  48894118                         mov qword ptr [rcx + 0x18], rax
000000014096e788  488d4128                         lea rax, [rcx + 0x28]
000000014096e78c  4c897908                         mov qword ptr [rcx + 8], r15
000000014096e790  488900                           mov qword ptr [rax], rax
000000014096e793  48894130                         mov qword ptr [rcx + 0x30], rax
000000014096e797  488d4140                         lea rax, [rcx + 0x40]
000000014096e79b  4c897920                         mov qword ptr [rcx + 0x20], r15
000000014096e79f  488900                           mov qword ptr [rax], rax
000000014096e7a2  48894148                         mov qword ptr [rcx + 0x48], rax
000000014096e7a6  488d4158                         lea rax, [rcx + 0x58]
000000014096e7aa  4c897938                         mov qword ptr [rcx + 0x38], r15
000000014096e7ae  488900                           mov qword ptr [rax], rax
000000014096e7b1  48894160                         mov qword ptr [rcx + 0x60], rax
000000014096e7b5  488d4170                         lea rax, [rcx + 0x70]
000000014096e7b9  4c897950                         mov qword ptr [rcx + 0x50], r15
000000014096e7bd  488900                           mov qword ptr [rax], rax
000000014096e7c0  48894178                         mov qword ptr [rcx + 0x78], rax
000000014096e7c4  488d8188000000                   lea rax, [rcx + 0x88]
000000014096e7cb  4c897968                         mov qword ptr [rcx + 0x68], r15
000000014096e7cf  488900                           mov qword ptr [rax], rax
000000014096e7d2  48898190000000                   mov qword ptr [rcx + 0x90], rax
000000014096e7d9  488d81a0000000                   lea rax, [rcx + 0xa0]
000000014096e7e0  4c89b980000000                   mov qword ptr [rcx + 0x80], r15
000000014096e7e7  488900                           mov qword ptr [rax], rax
000000014096e7ea  488981a8000000                   mov qword ptr [rcx + 0xa8], rax
000000014096e7f1  488d81b8000000                   lea rax, [rcx + 0xb8]
000000014096e7f8  4c89b998000000                   mov qword ptr [rcx + 0x98], r15
000000014096e7ff  488900                           mov qword ptr [rax], rax
000000014096e802  488981c0000000                   mov qword ptr [rcx + 0xc0], rax
000000014096e809  488d81d0000000                   lea rax, [rcx + 0xd0]
000000014096e810  4c89b9b0000000                   mov qword ptr [rcx + 0xb0], r15
000000014096e817  488900                           mov qword ptr [rax], rax
000000014096e81a  488981d8000000                   mov qword ptr [rcx + 0xd8], rax
000000014096e821  488d81e8000000                   lea rax, [rcx + 0xe8]
000000014096e828  4c89b9c8000000                   mov qword ptr [rcx + 0xc8], r15
000000014096e82f  488900                           mov qword ptr [rax], rax
000000014096e832  488981f0000000                   mov qword ptr [rcx + 0xf0], rax
000000014096e839  488d8100010000                   lea rax, [rcx + 0x100]
000000014096e840  4c89b9e0000000                   mov qword ptr [rcx + 0xe0], r15
000000014096e847  488900                           mov qword ptr [rax], rax
000000014096e84a  48898108010000                   mov qword ptr [rcx + 0x108], rax
000000014096e851  488d8118010000                   lea rax, [rcx + 0x118]
000000014096e858  4c89b9f8000000                   mov qword ptr [rcx + 0xf8], r15
000000014096e85f  488900                           mov qword ptr [rax], rax
000000014096e862  48898120010000                   mov qword ptr [rcx + 0x120], rax
000000014096e869  488d8130010000                   lea rax, [rcx + 0x130]
000000014096e870  4c89b910010000                   mov qword ptr [rcx + 0x110], r15
000000014096e877  488900                           mov qword ptr [rax], rax
000000014096e87a  48898138010000                   mov qword ptr [rcx + 0x138], rax
000000014096e881  488d8148010000                   lea rax, [rcx + 0x148]
000000014096e888  4c89b928010000                   mov qword ptr [rcx + 0x128], r15
000000014096e88f  488d8980010000                   lea rcx, [rcx + 0x180]
000000014096e896  488900                           mov qword ptr [rax], rax
000000014096e899  488941d0                         mov qword ptr [rcx - 0x30], rax
000000014096e89d  4c8979c0                         mov qword ptr [rcx - 0x40], r15
000000014096e8a1  4883ea01                         sub rdx, 1
000000014096e8a5  0f85b5feffff                     jne 0x14096e760
000000014096e8ab  488d8600110000                   lea rax, [rsi + 0x1100]
000000014096e8b2  b901080000                       mov ecx, 0x801
000000014096e8b7  660f1f840000000000               nop word ptr [rax + rax]
000000014096e8c0  488900                           mov qword ptr [rax], rax
000000014096e8c3  48894008                         mov qword ptr [rax + 8], rax
000000014096e8c7  4c8978f8                         mov qword ptr [rax - 8], r15
000000014096e8cb  4883c018                         add rax, 0x18
000000014096e8cf  4883e901                         sub rcx, 1
000000014096e8d3  75eb                             jne 0x14096e8c0
000000014096e8d5  83bec8e70000ff                   cmp dword ptr [rsi + 0xe7c8], -1
000000014096e8dc  7409                             je 0x14096e8e7
000000014096e8de  448b96cce70000                   mov r10d, dword ptr [rsi + 0xe7cc]
000000014096e8e5  eb1d                             jmp 0x14096e904
000000014096e8e7  8b86d4e70000                     mov eax, dword ptr [rsi + 0xe7d4]
000000014096e8ed  b960000000                       mov ecx, 0x60
000000014096e8f2  85c0                             test eax, eax
000000014096e8f4  0f4fc8                           cmovg ecx, eax
000000014096e8f7  8b86b0e70000                     mov eax, dword ptr [rsi + 0xe7b0]
000000014096e8fd  33d2                             xor edx, edx
000000014096e8ff  f7f1                             div ecx
000000014096e901  448bd0                           mov r10d, eax
000000014096e904  458bcf                           mov r9d, r15d
000000014096e907  4585d2                           test r10d, r10d
000000014096e90a  7e74                             jle 0x14096e980
000000014096e90c  0f1f4000                         nop dword ptr [rax]
000000014096e910  8b8ed4e70000                     mov ecx, dword ptr [rsi + 0xe7d4]
000000014096e916  ba60000000                       mov edx, 0x60
000000014096e91b  85c9                             test ecx, ecx
000000014096e91d  0f4fd1                           cmovg edx, ecx
000000014096e920  410fafd1                         imul edx, r9d
000000014096e924  4863d2                           movsxd rdx, edx
000000014096e927  480396a8e70000                   add rdx, qword ptr [rsi + 0xe7a8]
000000014096e92e  8b4228                           mov eax, dword ptr [rdx + 0x28]
000000014096e931  83f801                           cmp eax, 1
000000014096e934  750a                             jne 0x14096e940
000000014096e936  0fb74232                         movzx eax, word ptr [rdx + 0x32]
000000014096e93a  4883c035                         add rax, 0x35
000000014096e93e  eb13                             jmp 0x14096e953
000000014096e940  83f802                           cmp eax, 2
000000014096e943  0f85f7020000                     jne 0x14096ec40
000000014096e949  0fb74232                         movzx eax, word ptr [rdx + 0x32]
000000014096e94d  4805b5000000                     add rax, 0xb5
000000014096e953  4883c218                         add rdx, 0x18
000000014096e957  488d0440                         lea rax, [rax + rax*2]
000000014096e95b  4c8d04c6                         lea r8, [rsi + rax*8]
000000014096e95f  41ffc1                           inc r9d
000000014096e962  498b4010                         mov rax, qword ptr [r8 + 0x10]
000000014096e966  498d4808                         lea rcx, [r8 + 8]
000000014096e96a  48894208                         mov qword ptr [rdx + 8], rax
000000014096e96e  48890a                           mov qword ptr [rdx], rcx
000000014096e971  48895108                         mov qword ptr [rcx + 8], rdx
000000014096e975  488910                           mov qword ptr [rax], rdx
000000014096e978  49ff00                           inc qword ptr [r8]
000000014096e97b  453bca                           cmp r9d, r10d
000000014096e97e  7c90                             jl 0x14096e910
000000014096e980  83be00e8000000                   cmp dword ptr [rsi + 0xe800], 0
000000014096e987  7505                             jne 0x14096e98e
000000014096e989  418bc7                           mov eax, r15d
000000014096e98c  eb11                             jmp 0x14096e99f
000000014096e98e  488b8620040000                   mov rax, qword ptr [rsi + 0x420]
000000014096e995  482b8618040000                   sub rax, qword ptr [rsi + 0x418]
000000014096e99c  48d1f8                           sar rax, 1
000000014096e99f  4c63e8                           movsxd r13, eax
000000014096e9a2  4d8be7                           mov r12, r15
000000014096e9a5  85c0                             test eax, eax
000000014096e9a7  0f8e1d020000                     jle 0x14096ebca
000000014096e9ad  0f1f00                           nop dword ptr [rax]
000000014096e9b0  488b8618040000                   mov rax, qword ptr [rsi + 0x418]
000000014096e9b7  4c8b87f8d70200                   mov r8, qword ptr [rdi + 0x2d7f8]
000000014096e9be  420fbf0c60                       movsx ecx, word ptr [rax + r12*2]
000000014096e9c3  4103ce                           add ecx, r14d
000000014096e9c6  7907                             jns 0x14096e9cf
000000014096e9c8  b8ffffffff                       mov eax, 0xffffffff
000000014096e9cd  eb0b                             jmp 0x14096e9da
000000014096e9cf  8bc1                             mov eax, ecx
000000014096e9d1  99                               cdq
000000014096e9d2  83e27f                           and edx, 0x7f
000000014096e9d5  03c2                             add eax, edx
000000014096e9d7  c1f807                           sar eax, 7
000000014096e9da  4863d0                           movsxd rdx, eax
000000014096e9dd  81e17f000080                     and ecx, 0x8000007f
000000014096e9e3  7d07                             jge 0x14096e9ec
000000014096e9e5  ffc9                             dec ecx
000000014096e9e7  83c980                           or ecx, 0xffffff80
000000014096e9ea  ffc1                             inc ecx
000000014096e9ec  4863c1                           movsxd rax, ecx
000000014096e9ef  498b4cd018                       mov rcx, qword ptr [r8 + rdx*8 + 0x18]
000000014096e9f4  488b4cc118                       mov rcx, qword ptr [rcx + rax*8 + 0x18]
000000014096e9f9  83b908cf0100ff                   cmp dword ptr [rcx + 0x1cf08], -1
000000014096ea00  7409                             je 0x14096ea0b
000000014096ea02  448b990ccf0100                   mov r11d, dword ptr [rcx + 0x1cf0c]
000000014096ea09  eb20                             jmp 0x14096ea2b
000000014096ea0b  8b8114cf0100                     mov eax, dword ptr [rcx + 0x1cf14]
000000014096ea11  41b860000000                     mov r8d, 0x60
000000014096ea17  85c0                             test eax, eax
000000014096ea19  440f4fc0                         cmovg r8d, eax
000000014096ea1d  8b81f0ce0100                     mov eax, dword ptr [rcx + 0x1cef0]
000000014096ea23  33d2                             xor edx, edx
000000014096ea25  41f7f0                           div r8d
000000014096ea28  448bd8                           mov r11d, eax
000000014096ea2b  458bd7                           mov r10d, r15d
000000014096ea2e  4585db                           test r11d, r11d
000000014096ea31  0f8e7c000000                     jle 0x14096eab3
000000014096ea37  660f1f840000000000               nop word ptr [rax + rax]
000000014096ea40  8b9114cf0100                     mov edx, dword ptr [rcx + 0x1cf14]
000000014096ea46  b860000000                       mov eax, 0x60
000000014096ea4b  85d2                             test edx, edx
000000014096ea4d  0f4fc2                           cmovg eax, edx
000000014096ea50  410fafc2                         imul eax, r10d
000000014096ea54  4c63c0                           movsxd r8, eax
000000014096ea57  4c0381e8ce0100                   add r8, qword ptr [rcx + 0x1cee8]
000000014096ea5e  418b4028                         mov eax, dword ptr [r8 + 0x28]
000000014096ea62  83f801                           cmp eax, 1
000000014096ea65  750b                             jne 0x14096ea72
000000014096ea67  410fb74032                       movzx eax, word ptr [r8 + 0x32]
000000014096ea6c  4883c035                         add rax, 0x35
000000014096ea70  eb14                             jmp 0x14096ea86
000000014096ea72  83f802                           cmp eax, 2
000000014096ea75  0f85c5010000                     jne 0x14096ec40
000000014096ea7b  410fb74032                       movzx eax, word ptr [r8 + 0x32]
000000014096ea80  4805b5000000                     add rax, 0xb5
000000014096ea86  4983c018                         add r8, 0x18
000000014096ea8a  488d0440                         lea rax, [rax + rax*2]
000000014096ea8e  4c8d0cc6                         lea r9, [rsi + rax*8]
000000014096ea92  41ffc2                           inc r10d
000000014096ea95  498b4110                         mov rax, qword ptr [r9 + 0x10]
000000014096ea99  498d5108                         lea rdx, [r9 + 8]
000000014096ea9d  49894008                         mov qword ptr [r8 + 8], rax
000000014096eaa1  498910                           mov qword ptr [r8], rdx
000000014096eaa4  4c894208                         mov qword ptr [rdx + 8], r8
000000014096eaa8  4c8900                           mov qword ptr [rax], r8
000000014096eaab  49ff01                           inc qword ptr [r9]
000000014096eaae  453bd3                           cmp r10d, r11d
000000014096eab1  7c8d                             jl 0x14096ea40
000000014096eab3  488b8178130100                   mov rax, qword ptr [rcx + 0x11378]
000000014096eaba  498bef                           mov rbp, r15
000000014096eabd  482b8170130100                   sub rax, qword ptr [rcx + 0x11370]
000000014096eac4  48c1f803                         sar rax, 3
000000014096eac8  4c63f8                           movsxd r15, eax
000000014096eacb  85c0                             test eax, eax
000000014096eacd  0f8ed5000000                     jle 0x14096eba8
000000014096ead3  488b8170130100                   mov rax, qword ptr [rcx + 0x11370]
000000014096eada  488b3ce8                         mov rdi, qword ptr [rax + rbp*8]
000000014096eade  83bf08010000ff                   cmp dword ptr [rdi + 0x108], -1
000000014096eae5  7409                             je 0x14096eaf0
000000014096eae7  448b9f0c010000                   mov r11d, dword ptr [rdi + 0x10c]
000000014096eaee  eb20                             jmp 0x14096eb10
000000014096eaf0  8b8714010000                     mov eax, dword ptr [rdi + 0x114]
000000014096eaf6  41b860000000                     mov r8d, 0x60
000000014096eafc  85c0                             test eax, eax
000000014096eafe  440f4fc0                         cmovg r8d, eax
000000014096eb02  8b87f0000000                     mov eax, dword ptr [rdi + 0xf0]
000000014096eb08  33d2                             xor edx, edx
000000014096eb0a  41f7f0                           div r8d
000000014096eb0d  448bd8                           mov r11d, eax
000000014096eb10  33d2                             xor edx, edx
000000014096eb12  4585db                           test r11d, r11d
000000014096eb15  0f8e79000000                     jle 0x14096eb94
000000014096eb1b  0f1f440000                       nop dword ptr [rax + rax]
000000014096eb20  8b8714010000                     mov eax, dword ptr [rdi + 0x114]
000000014096eb26  41b860000000                     mov r8d, 0x60
000000014096eb2c  85c0                             test eax, eax
000000014096eb2e  440f4fc0                         cmovg r8d, eax
000000014096eb32  440fafc2                         imul r8d, edx
000000014096eb36  4d63c8                           movsxd r9, r8d
000000014096eb39  4c038fe8000000                   add r9, qword ptr [rdi + 0xe8]
000000014096eb40  418b4128                         mov eax, dword ptr [r9 + 0x28]
000000014096eb44  83f801                           cmp eax, 1
000000014096eb47  750b                             jne 0x14096eb54
000000014096eb49  410fb74132                       movzx eax, word ptr [r9 + 0x32]
000000014096eb4e  4883c035                         add rax, 0x35
000000014096eb52  eb14                             jmp 0x14096eb68
000000014096eb54  83f802                           cmp eax, 2
000000014096eb57  0f85e3000000                     jne 0x14096ec40
000000014096eb5d  410fb74132                       movzx eax, word ptr [r9 + 0x32]
000000014096eb62  4805b5000000                     add rax, 0xb5
000000014096eb68  4983c118                         add r9, 0x18
000000014096eb6c  488d0440                         lea rax, [rax + rax*2]
000000014096eb70  4c8d14c6                         lea r10, [rsi + rax*8]
000000014096eb74  ffc2                             inc edx
000000014096eb76  498b4210                         mov rax, qword ptr [r10 + 0x10]
000000014096eb7a  4d8d4208                         lea r8, [r10 + 8]
000000014096eb7e  49894108                         mov qword ptr [r9 + 8], rax
000000014096eb82  4d8901                           mov qword ptr [r9], r8
000000014096eb85  4d894808                         mov qword ptr [r8 + 8], r9
000000014096eb89  4c8908                           mov qword ptr [rax], r9
000000014096eb8c  49ff02                           inc qword ptr [r10]
000000014096eb8f  413bd3                           cmp edx, r11d
000000014096eb92  7c8c                             jl 0x14096eb20
000000014096eb94  48ffc5                           inc rbp
000000014096eb97  493bef                           cmp rbp, r15
000000014096eb9a  0f8c33ffffff                     jl 0x14096ead3
000000014096eba0  448bb42488000000                 mov r14d, dword ptr [rsp + 0x88]
000000014096eba8  488bbc2480000000                 mov rdi, qword ptr [rsp + 0x80]
000000014096ebb0  49ffc4                           inc r12
000000014096ebb3  41bf00000000                     mov r15d, 0
000000014096ebb9  4d3be5                           cmp r12, r13
000000014096ebbc  0f8ceefdffff                     jl 0x14096e9b0
000000014096ebc2  488bac2490000000                 mov rbp, qword ptr [rsp + 0x90]
000000014096ebca  488d8ea8040000                   lea rcx, [rsi + 0x4a8]
000000014096ebd1  e8ff8aa503                       call 0x1443c76d5
000000014096ebd6  488b8c2498000000                 mov rcx, qword ptr [rsp + 0x98]
000000014096ebde  4183ee80                         sub r14d, -0x80
000000014096ebe2  4883c108                         add rcx, 8
000000014096ebe6  4489b42488000000                 mov dword ptr [rsp + 0x88], r14d
000000014096ebee  48ffc5                           inc rbp
000000014096ebf1  48898c2498000000                 mov qword ptr [rsp + 0x98], rcx
000000014096ebf9  4889ac2490000000                 mov qword ptr [rsp + 0x90], rbp
000000014096ec01  483b6c2420                       cmp rbp, qword ptr [rsp + 0x20]
000000014096ec06  0f8c14fbffff                     jl 0x14096e720
000000014096ec0c  488b8ff81b0100                   mov rcx, qword ptr [rdi + 0x11bf8]
000000014096ec13  488d942488000000                 lea rdx, [rsp + 0x88]
000000014096ec1b  4881c100120000                   add rcx, 0x1200
000000014096ec22  4489bc2488000000                 mov dword ptr [rsp + 0x88], r15d
000000014096ec2a  e821f3d9ff                       call 0x14070df50
000000014096ec2f  4883c438                         add rsp, 0x38
000000014096ec33  415f                             pop r15
000000014096ec35  415e                             pop r14
000000014096ec37  415d                             pop r13
000000014096ec39  415c                             pop r12
000000014096ec3b  5f                               pop rdi
000000014096ec3c  5e                               pop rsi
000000014096ec3d  5d                               pop rbp
000000014096ec3e  5b                               pop rbx
000000014096ec3f  c3                               ret
000000014096ec40  488d8ea8040000                   lea rcx, [rsi + 0x4a8]
000000014096ec47  e8898aa503                       call 0x1443c76d5
000000014096ec4c  ebe1                             jmp 0x14096ec2f
000000014096ec4e  ffc8                             dec eax
000000014096ec50  b906000000                       mov ecx, 6
000000014096ec55  8986f4040000                     mov dword ptr [rsi + 0x4f4], eax
000000014096ec5b  e8818aa503                       call 0x1443c76e1
000000014096ec60  cc                               int3
000000014096ec61  b905000000                       mov ecx, 5
000000014096ec66  e8768aa503                       call 0x1443c76e1
000000014096ec6b  cc                               int3
000000014096ec6c  cc                               int3
000000014096ec6d  cc                               int3
000000014096ec6e  cc                               int3
000000014096ec6f  cc                               int3
000000014096ec70  48895c2410                       mov qword ptr [rsp + 0x10], rbx
000000014096ec75  48896c2418                       mov qword ptr [rsp + 0x18], rbp
000000014096ec7a  4889742420                       mov qword ptr [rsp + 0x20], rsi
000000014096ec7f  57                               push rdi
000000014096ec80  4156                             push r14
000000014096ec82  4157                             push r15
000000014096ec84  4881ec80000000                   sub rsp, 0x80
000000014096ec8b  488be9                           mov rbp, rcx
000000014096ec8e  4533c0                           xor r8d, r8d
000000014096ec91  b201                             mov dl, 1
000000014096ec93  e8d8c6ffff                       call 0x14096b370
000000014096ec98  488b8528400100                   mov rax, qword ptr [rbp + 0x14028]
000000014096ec9f  4885c0                           test rax, rax
000000014096eca2  7451                             je 0x14096ecf5
000000014096eca4  83781cff                         cmp dword ptr [rax + 0x1c], -1
000000014096eca8  7542                             jne 0x14096ecec
000000014096ecaa  48634820                         movsxd rcx, dword ptr [rax + 0x20]
000000014096ecae  488d1449                         lea rdx, [rcx + rcx*2]
000000014096ecb2  48c1e206                         shl rdx, 6
000000014096ecb6  488b8df8d70200                   mov rcx, qword ptr [rbp + 0x2d7f8]
000000014096ecbd  c6840aba08000001                 mov byte ptr [rdx + rcx + 0x8ba], 1

; automation_host_callback
0000000140958d50  48895c2418                       mov qword ptr [rsp + 0x18], rbx
0000000140958d55  48896c2420                       mov qword ptr [rsp + 0x20], rbp
0000000140958d5a  56                               push rsi
0000000140958d5b  57                               push rdi
0000000140958d5c  4155                             push r13
0000000140958d5e  4156                             push r14
0000000140958d60  4157                             push r15
0000000140958d62  4881eca0000000                   sub rsp, 0xa0
0000000140958d69  488ba9181c0100                   mov rbp, qword ptr [rcx + 0x11c18]
0000000140958d70  33ff                             xor edi, edi
0000000140958d72  448bac2400010000                 mov r13d, dword ptr [rsp + 0x100]
0000000140958d7a  418bd8                           mov ebx, r8d
0000000140958d7d  0f29bc2480000000                 movaps xmmword ptr [rsp + 0x80], xmm7
0000000140958d85  488bf1                           mov rsi, rcx
0000000140958d88  4d63f1                           movsxd r14, r9d
0000000140958d8b  0f28f9                           movaps xmm7, xmm1
0000000140958d8e  41bfffffffff                     mov r15d, 0xffffffff
0000000140958d94  397d00                           cmp dword ptr [rbp], edi
0000000140958d97  7473                             je 0x140958e0c
0000000140958d99  397d04                           cmp dword ptr [rbp + 4], edi
0000000140958d9c  7c6e                             jl 0x140958e0c
0000000140958d9e  e85ef8a603                       call 0x1443c8601
0000000140958da3  482b4510                         sub rax, qword ptr [rbp + 0x10]
0000000140958da7  483d404b4c00                     cmp rax, 0x4c4b40
0000000140958dad  7402                             je 0x140958db1
0000000140958daf  7d5b                             jge 0x140958e0c
0000000140958db1  488b86181c0100                   mov rax, qword ptr [rsi + 0x11c18]
0000000140958db8  bd01000000                       mov ebp, 1
0000000140958dbd  448b8c24f0000000                 mov r9d, dword ptr [rsp + 0xf0]
0000000140958dc5  458bc6                           mov r8d, r14d
0000000140958dc8  c644244001                       mov byte ptr [rsp + 0x40], 1
0000000140958dcd  8bd3                             mov edx, ebx
0000000140958dcf  488bce                           mov rcx, rsi
0000000140958dd2  0fb74004                         movzx eax, word ptr [rax + 4]
0000000140958dd6  6689442438                       mov word ptr [rsp + 0x38], ax
0000000140958ddb  8b8424f8000000                   mov eax, dword ptr [rsp + 0xf8]
0000000140958de2  896c2430                         mov dword ptr [rsp + 0x30], ebp
0000000140958de6  44896c2428                       mov dword ptr [rsp + 0x28], r13d
0000000140958deb  89442420                         mov dword ptr [rsp + 0x20], eax
0000000140958def  e80ca6f9ff                       call 0x1408f3400
0000000140958df4  488b86181c0100                   mov rax, qword ptr [rsi + 0x11c18]
0000000140958dfb  3928                             cmp dword ptr [rax], ebp
0000000140958dfd  7502                             jne 0x140958e01
0000000140958dff  8938                             mov dword ptr [rax], edi
0000000140958e01  44897804                         mov dword ptr [rax + 4], r15d
0000000140958e05  4086aea1200100                   xchg byte ptr [rsi + 0x120a1], bpl
0000000140958e0c  418d8561fdffff                   lea eax, [r13 - 0x29f]
0000000140958e13  4c89a424d8000000                 mov qword ptr [rsp + 0xd8], r12
0000000140958e1b  83f806                           cmp eax, 6
0000000140958e1e  8bc3                             mov eax, ebx
0000000140958e20  7740                             ja 0x140958e62
0000000140958e22  c1e807                           shr eax, 7
0000000140958e25  85db                             test ebx, ebx
0000000140958e27  410f48c7                         cmovs eax, r15d
0000000140958e2b  4863c8                           movsxd rcx, eax
0000000140958e2e  488b86f8d70200                   mov rax, qword ptr [rsi + 0x2d7f8]
0000000140958e35  488b54c818                       mov rdx, qword ptr [rax + rcx*8 + 0x18]
0000000140958e3a  4c8da2a8e70000                   lea r12, [rdx + 0xe7a8]
0000000140958e41  4c8d8ab0e70000                   lea r9, [rdx + 0xe7b0]
0000000140958e48  4c8dbad4e70000                   lea r15, [rdx + 0xe7d4]
0000000140958e4f  488d8acce70000                   lea rcx, [rdx + 0xe7cc]
0000000140958e56  4881c2c8e70000                   add rdx, 0xe7c8
0000000140958e5d  e986000000                       jmp 0x140958ee8
0000000140958e62  488b96f8d70200                   mov rdx, qword ptr [rsi + 0x2d7f8]
0000000140958e69  c1e807                           shr eax, 7
0000000140958e6c  85db                             test ebx, ebx
0000000140958e6e  410f48c7                         cmovs eax, r15d
0000000140958e72  4898                             cdqe
0000000140958e74  81e37f000080                     and ebx, 0x8000007f
0000000140958e7a  7d07                             jge 0x140958e83
0000000140958e7c  ffcb                             dec ebx
0000000140958e7e  83cb80                           or ebx, 0xffffff80
0000000140958e81  ffc3                             inc ebx
0000000140958e83  488b44c218                       mov rax, qword ptr [rdx + rax*8 + 0x18]
0000000140958e88  4863cb                           movsxd rcx, ebx
0000000140958e8b  488b44c818                       mov rax, qword ptr [rax + rcx*8 + 0x18]
0000000140958e90  4585f6                           test r14d, r14d
0000000140958e93  7925                             jns 0x140958eba
0000000140958e95  4c8da0e8ce0100                   lea r12, [rax + 0x1cee8]
0000000140958e9c  4c8d88f0ce0100                   lea r9, [rax + 0x1cef0]
0000000140958ea3  4c8db814cf0100                   lea r15, [rax + 0x1cf14]
0000000140958eaa  488d880ccf0100                   lea rcx, [rax + 0x1cf0c]
0000000140958eb1  488d9008cf0100                   lea rdx, [rax + 0x1cf08]
0000000140958eb8  eb2e                             jmp 0x140958ee8
0000000140958eba  488b8870130100                   mov rcx, qword ptr [rax + 0x11370]
0000000140958ec1  4a8b04f1                         mov rax, qword ptr [rcx + r14*8]
0000000140958ec5  4c8da0e8000000                   lea r12, [rax + 0xe8]
0000000140958ecc  4c8d88f0000000                   lea r9, [rax + 0xf0]
0000000140958ed3  4c8db814010000                   lea r15, [rax + 0x114]
0000000140958eda  488d880c010000                   lea rcx, [rax + 0x10c]
0000000140958ee1  488d9008010000                   lea rdx, [rax + 0x108]
0000000140958ee8  4032ed                           xor bpl, bpl
0000000140958eeb  833aff                           cmp dword ptr [rdx], -1
0000000140958eee  7405                             je 0x140958ef5
0000000140958ef0  448b31                           mov r14d, dword ptr [rcx]
0000000140958ef3  eb1a                             jmp 0x140958f0f
0000000140958ef5  418b0f                           mov ecx, dword ptr [r15]
0000000140958ef8  41b860000000                     mov r8d, 0x60
0000000140958efe  418b01                           mov eax, dword ptr [r9]
0000000140958f01  85c9                             test ecx, ecx
0000000140958f03  440f4fc1                         cmovg r8d, ecx
0000000140958f07  33d2                             xor edx, edx
0000000140958f09  41f7f0                           div r8d
0000000140958f0c  448bf0                           mov r14d, eax
0000000140958f0f  8b156bbd6704                     mov edx, dword ptr [rip + 0x467bd6b]
0000000140958f15  899424d0000000                   mov dword ptr [rsp + 0xd0], edx
0000000140958f1c  4585f6                           test r14d, r14d
0000000140958f1f  0f8e45010000                     jle 0x14095906a
0000000140958f25  440f29442470                     movaps xmmword ptr [rsp + 0x70], xmm8
0000000140958f2b  f3440f1005a02bdb03               movss xmm8, dword ptr [rip + 0x3db2ba0]
0000000140958f34  0f29b42490000000                 movaps xmmword ptr [rsp + 0x90], xmm6
0000000140958f3c  0f1f4000                         nop dword ptr [rax]
0000000140958f40  418b07                           mov eax, dword ptr [r15]
0000000140958f43  b960000000                       mov ecx, 0x60
0000000140958f48  85c0                             test eax, eax
0000000140958f4a  0f4fc8                           cmovg ecx, eax
0000000140958f4d  0fafcf                           imul ecx, edi
0000000140958f50  4863d9                           movsxd rbx, ecx
0000000140958f53  49031c24                         add rbx, qword ptr [r12]
0000000140958f57  837b2802                         cmp dword ptr [rbx + 0x28], 2
0000000140958f5b  750c                             jne 0x140958f69
0000000140958f5d  0fb74332                         movzx eax, word ptr [rbx + 0x32]
0000000140958f61  3bc2                             cmp eax, edx
0000000140958f63  0f8de8000000                     jge 0x140959051
0000000140958f69  8b4340                           mov eax, dword ptr [rbx + 0x40]
0000000140958f6c  85c0                             test eax, eax
0000000140958f6e  780d                             js 0x140958f7d
0000000140958f70  3b8424f0000000                   cmp eax, dword ptr [rsp + 0xf0]
0000000140958f77  0f85d4000000                     jne 0x140959051
0000000140958f7d  44396b34                         cmp dword ptr [rbx + 0x34], r13d
0000000140958f81  0f85ca000000                     jne 0x140959051
0000000140958f87  837b3c00                         cmp dword ptr [rbx + 0x3c], 0
0000000140958f8b  7d10                             jge 0x140958f9d
0000000140958f8d  8b8424f8000000                   mov eax, dword ptr [rsp + 0xf8]
0000000140958f94  394344                           cmp dword ptr [rbx + 0x44], eax
0000000140958f97  0f85b4000000                     jne 0x140959051
0000000140958f9d  837b5800                         cmp dword ptr [rbx + 0x58], 0
0000000140958fa1  0f28d7                           movaps xmm2, xmm7
0000000140958fa4  7f13                             jg 0x140958fb9
0000000140958fa6  f30f104354                       movss xmm0, dword ptr [rbx + 0x54]
0000000140958fab  f30f5c4350                       subss xmm0, dword ptr [rbx + 0x50]
0000000140958fb0  f30f5c5350                       subss xmm2, dword ptr [rbx + 0x50]
0000000140958fb5  f30f5ed0                         divss xmm2, xmm0
0000000140958fb9  0f28c2                           movaps xmm0, xmm2
0000000140958fbc  0f57c9                           xorps xmm1, xmm1
0000000140958fbf  0f57f6                           xorps xmm6, xmm6
0000000140958fc2  0f57d2                           xorps xmm2, xmm2
0000000140958fc5  f3410f10f0                       movss xmm6, xmm8
0000000140958fca  f30f10d0                         movss xmm2, xmm0
0000000140958fce  f30f5fca                         maxss xmm1, xmm2
0000000140958fd2  488bcb                           mov rcx, rbx
0000000140958fd5  f30f5df1                         minss xmm6, xmm1
0000000140958fd9  0f28ce                           movaps xmm1, xmm6
0000000140958fdc  e87ff40000                       call 0x140968460
0000000140958fe1  837b2802                         cmp dword ptr [rbx + 0x28], 2
0000000140958fe5  7563                             jne 0x14095904a
0000000140958fe7  4084ed                           test bpl, bpl
0000000140958fea  755e                             jne 0x14095904a
0000000140958fec  8b842410010000                   mov eax, dword ptr [rsp + 0x110]
0000000140958ff3  40b501                           mov bpl, 1
0000000140958ff6  89442450                         mov dword ptr [rsp + 0x50], eax
0000000140958ffa  8b842408010000                   mov eax, dword ptr [rsp + 0x108]
0000000140959001  f30f11732c                       movss dword ptr [rbx + 0x2c], xmm6
0000000140959006  488b8e78d80200                   mov rcx, qword ptr [rsi + 0x2d878]
000000014095900d  89442458                         mov dword ptr [rsp + 0x58], eax
0000000140959011  0fb74332                         movzx eax, word ptr [rbx + 0x32]
0000000140959015  6689842400010000                 mov word ptr [rsp + 0x100], ax
000000014095901d  f30f11742460                     movss dword ptr [rsp + 0x60], xmm6
0000000140959023  4885c9                           test rcx, rcx
0000000140959026  746b                             je 0x140959093
0000000140959028  488b01                           mov rax, qword ptr [rcx]
000000014095902b  488d542450                       lea rdx, [rsp + 0x50]
0000000140959030  4889542420                       mov qword ptr [rsp + 0x20], rdx
0000000140959035  4c8d4c2458                       lea r9, [rsp + 0x58]
000000014095903a  488d942400010000                 lea rdx, [rsp + 0x100]
0000000140959042  4c8d442460                       lea r8, [rsp + 0x60]
0000000140959047  ff5010                           call qword ptr [rax + 0x10]
000000014095904a  8b9424d0000000                   mov edx, dword ptr [rsp + 0xd0]
0000000140959051  ffc7                             inc edi
0000000140959053  413bfe                           cmp edi, r14d
0000000140959056  0f8ce4feffff                     jl 0x140958f40
000000014095905c  0f28b42490000000                 movaps xmm6, xmmword ptr [rsp + 0x90]
0000000140959064  440f28442470                     movaps xmm8, xmmword ptr [rsp + 0x70]
000000014095906a  4c8ba424d8000000                 mov r12, qword ptr [rsp + 0xd8]
0000000140959072  4c8d9c24a0000000                 lea r11, [rsp + 0xa0]
000000014095907a  498b5b40                         mov rbx, qword ptr [r11 + 0x40]
000000014095907e  498b6b48                         mov rbp, qword ptr [r11 + 0x48]
0000000140959082  410f287be0                       movaps xmm7, xmmword ptr [r11 - 0x20]
0000000140959087  498be3                           mov rsp, r11
000000014095908a  415f                             pop r15
000000014095908c  415e                             pop r14
000000014095908e  415d                             pop r13
0000000140959090  5f                               pop rdi
0000000140959091  5e                               pop rsi
0000000140959092  c3                               ret
0000000140959093  e8b8e6a603                       call 0x1443c7750
0000000140959098  cc                               int3
0000000140959099  cc                               int3
000000014095909a  cc                               int3
000000014095909b  cc                               int3
000000014095909c  cc                               int3
000000014095909d  cc                               int3
000000014095909e  cc                               int3
000000014095909f  cc                               int3
00000001409590a0  48895c2408                       mov qword ptr [rsp + 8], rbx
00000001409590a5  57                               push rdi
00000001409590a6  4881ec80000000                   sub rsp, 0x80
00000001409590ad  0f29742470                       movaps xmmword ptr [rsp + 0x70], xmm6
00000001409590b2  0f28c2                           movaps xmm0, xmm2
00000001409590b5  0f297c2460                       movaps xmmword ptr [rsp + 0x60], xmm7
00000001409590ba  488bd9                           mov rbx, rcx
00000001409590bd  440f29442450                     movaps xmmword ptr [rsp + 0x50], xmm8
00000001409590c3  440f28c3                         movaps xmm8, xmm3
00000001409590c7  440f294c2440                     movaps xmmword ptr [rsp + 0x40], xmm9
00000001409590cd  440f29542430                     movaps xmmword ptr [rsp + 0x30], xmm10
00000001409590d3  4863fa                           movsxd rdi, edx
00000001409590d6  e8d59af9ff                       call 0x1408f2bb0
00000001409590db  f3440f5c052470e003               subss xmm8, dword ptr [rip + 0x3e07024]
00000001409590e4  f3440f100de729db03               movss xmm9, dword ptr [rip + 0x3db29e7]
00000001409590ed  0f57ff                           xorps xmm7, xmm7
00000001409590f0  440f28d0                         movaps xmm10, xmm0
00000001409590f4  f3440f590587f65d04               mulss xmm8, dword ptr [rip + 0x45df687]
00000001409590fd  440f2fc7                         comiss xmm8, xmm7
0000000140959101  7306                             jae 0x140959109
0000000140959103  450f57c0                         xorps xmm8, xmm8
0000000140959107  eb05                             jmp 0x14095910e
0000000140959109  f3450f5dc1                       minss xmm8, xmm9
000000014095910e  f30f10b424b0000000               movss xmm6, dword ptr [rsp + 0xb0]
0000000140959117  488d8bc8010000                   lea rcx, [rbx + 0x1c8]
000000014095911e  f30f5c35eafc5d04                 subss xmm6, dword ptr [rip + 0x45dfcea]
0000000140959126  4869ff60010000                   imul rdi, rdi, 0x160
000000014095912d  410f28d0                         movaps xmm2, xmm8
0000000140959131  f30f593563f55d04                 mulss xmm6, dword ptr [rip + 0x45df563]
0000000140959139  410f28ca                         movaps xmm1, xmm10
000000014095913d  f3410f5cf1                       subss xmm6, xmm9
0000000140959142  c644242007                       mov byte ptr [rsp + 0x20], 7
0000000140959147  4803cf                           add rcx, rdi
000000014095914a  f30f5f3502b9db03                 maxss xmm6, dword ptr [rip + 0x3dbb902]
0000000140959152  f3410f5df1                       minss xmm6, xmm9
0000000140959157  0f28de                           movaps xmm3, xmm6
000000014095915a  e861000000                       call 0x1409591c0
000000014095915f  440f2fd7                         comiss xmm10, xmm7
0000000140959163  7209                             jb 0x14095916e
0000000140959165  410f28fa                         movaps xmm7, xmm10
0000000140959169  f3410f5df9                       minss xmm7, xmm9

; program_private_reader
0000000140d0a2c0  4053                             push rbx
0000000140d0a2c2  55                               push rbp
0000000140d0a2c3  56                               push rsi
0000000140d0a2c4  57                               push rdi
0000000140d0a2c5  4154                             push r12
0000000140d0a2c7  4156                             push r14
0000000140d0a2c9  4157                             push r15
0000000140d0a2cb  4881ec70010000                   sub rsp, 0x170
0000000140d0a2d2  488b05e7b55909                   mov rax, qword ptr [rip + 0x959b5e7]
0000000140d0a2d9  4833c4                           xor rax, rsp
0000000140d0a2dc  4889842468010000                 mov qword ptr [rsp + 0x168], rax
0000000140d0a2e4  498be9                           mov rbp, r9
0000000140d0a2e7  450fb7e0                         movzx r12d, r8w
0000000140d0a2eb  488bf2                           mov rsi, rdx
0000000140d0a2ee  488bf9                           mov rdi, rcx
0000000140d0a2f1  33d2                             xor edx, edx
0000000140d0a2f3  488b8918130100                   mov rcx, qword ptr [rcx + 0x11318]
0000000140d0a2fa  e891defdff                       call 0x140ce8190
0000000140d0a2ff  448bf0                           mov r14d, eax
0000000140d0a302  ba01000000                       mov edx, 1
0000000140d0a307  488b8f18130100                   mov rcx, qword ptr [rdi + 0x11318]
0000000140d0a30e  e87ddefdff                       call 0x140ce8190
0000000140d0a313  448bf8                           mov r15d, eax
0000000140d0a316  418d4c2480                       lea ecx, [r12 - 0x80]
0000000140d0a31b  83f935                           cmp ecx, 0x35
0000000140d0a31e  0f87d8210000                     ja 0x140d0c4fc
0000000140d0a324  4863c9                           movsxd rcx, ecx
0000000140d0a327  488d05d25c2fff                   lea rax, [rip - 0xd0a32e]
0000000140d0a32e  0fb68c0850c5d000                 movzx ecx, byte ptr [rax + rcx + 0xd0c550]
0000000140d0a336  8b948818c5d000                   mov edx, dword ptr [rax + rcx*4 + 0xd0c518]
0000000140d0a33d  4803d0                           add rdx, rax
0000000140d0a340  ffe2                             jmp rdx
0000000140d0a342  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0a349  4c8bc5                           mov r8, rbp
0000000140d0a34c  488bce                           mov rcx, rsi
0000000140d0a34f  e86c60feff                       call 0x140cf03c0
0000000140d0a354  488bce                           mov rcx, rsi
0000000140d0a357  e864ddd601                       call 0x142a780c0
0000000140d0a35c  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0a362  488bce                           mov rcx, rsi
0000000140d0a365  e856ddd601                       call 0x142a780c0
0000000140d0a36a  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0a370  488bce                           mov rcx, rsi
0000000140d0a373  e8f8ded601                       call 0x142a78270
0000000140d0a378  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0a37e  488bce                           mov rcx, rsi
0000000140d0a381  e83addd601                       call 0x142a780c0
0000000140d0a386  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0a38c  488bce                           mov rcx, rsi
0000000140d0a38f  e8dcded601                       call 0x142a78270
0000000140d0a394  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0a39a  488bce                           mov rcx, rsi
0000000140d0a39d  e8ceded601                       call 0x142a78270
0000000140d0a3a2  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0a3a8  488bce                           mov rcx, rsi
0000000140d0a3ab  e810ddd601                       call 0x142a780c0
0000000140d0a3b0  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0a3b6  488bce                           mov rcx, rsi
0000000140d0a3b9  e8b2ded601                       call 0x142a78270
0000000140d0a3be  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0a3c4  488bce                           mov rcx, rsi
0000000140d0a3c7  e8f4dcd601                       call 0x142a780c0
0000000140d0a3cc  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0a3d2  488bce                           mov rcx, rsi
0000000140d0a3d5  e8e6dcd601                       call 0x142a780c0
0000000140d0a3da  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0a3e0  488bce                           mov rcx, rsi
0000000140d0a3e3  e8d8dcd601                       call 0x142a780c0
0000000140d0a3e8  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0a3ee  488bce                           mov rcx, rsi
0000000140d0a3f1  e87aded601                       call 0x142a78270
0000000140d0a3f6  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0a3fc  488bce                           mov rcx, rsi
0000000140d0a3ff  e8bcdcd601                       call 0x142a780c0
0000000140d0a404  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0a40a  488bce                           mov rcx, rsi
0000000140d0a40d  e8aedcd601                       call 0x142a780c0
0000000140d0a412  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0a418  488bce                           mov rcx, rsi
0000000140d0a41b  e8a0dcd601                       call 0x142a780c0
0000000140d0a420  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0a426  488bce                           mov rcx, rsi
0000000140d0a429  e892dcd601                       call 0x142a780c0
0000000140d0a42e  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0a434  488bce                           mov rcx, rsi
0000000140d0a437  e884dcd601                       call 0x142a780c0
0000000140d0a43c  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0a442  488bce                           mov rcx, rsi
0000000140d0a445  e876dcd601                       call 0x142a780c0
0000000140d0a44a  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0a450  488bce                           mov rcx, rsi
0000000140d0a453  e868dcd601                       call 0x142a780c0
0000000140d0a458  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0a45e  488bce                           mov rcx, rsi
0000000140d0a461  e85adcd601                       call 0x142a780c0
0000000140d0a466  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0a46c  488bce                           mov rcx, rsi
0000000140d0a46f  e84cdcd601                       call 0x142a780c0
0000000140d0a474  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0a47a  488bce                           mov rcx, rsi
0000000140d0a47d  e83edcd601                       call 0x142a780c0
0000000140d0a482  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0a488  488bce                           mov rcx, rsi
0000000140d0a48b  e830dcd601                       call 0x142a780c0
0000000140d0a490  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0a496  488bce                           mov rcx, rsi
0000000140d0a499  e822dcd601                       call 0x142a780c0
0000000140d0a49e  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0a4a4  488bce                           mov rcx, rsi
0000000140d0a4a7  e814dcd601                       call 0x142a780c0
0000000140d0a4ac  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0a4b2  488bce                           mov rcx, rsi
0000000140d0a4b5  e8b6ddd601                       call 0x142a78270
0000000140d0a4ba  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0a4c0  488bce                           mov rcx, rsi
0000000140d0a4c3  e8a8ddd601                       call 0x142a78270
0000000140d0a4c8  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0a4ce  488bce                           mov rcx, rsi
0000000140d0a4d1  e89addd601                       call 0x142a78270
0000000140d0a4d6  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0a4dc  488bce                           mov rcx, rsi
0000000140d0a4df  e8dcdbd601                       call 0x142a780c0
0000000140d0a4e4  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0a4ea  488bce                           mov rcx, rsi
0000000140d0a4ed  e8cedbd601                       call 0x142a780c0
0000000140d0a4f2  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0a4f8  488bce                           mov rcx, rsi
0000000140d0a4fb  e820ddd601                       call 0x142a78220
0000000140d0a500  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0a507  488bce                           mov rcx, rsi
0000000140d0a50a  e8b1dbd601                       call 0x142a780c0
0000000140d0a50f  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0a515  488d97b00d0100                   lea rdx, [rdi + 0x10db0]
0000000140d0a51c  4c8bc5                           mov r8, rbp
0000000140d0a51f  488bce                           mov rcx, rsi
0000000140d0a522  e8a911ffff                       call 0x140cfb6d0
0000000140d0a527  4c8bc5                           mov r8, rbp
0000000140d0a52a  488d97a8c00100                   lea rdx, [rdi + 0x1c0a8]
0000000140d0a531  488bce                           mov rcx, rsi
0000000140d0a534  e88731c3ff                       call 0x14093d6c0
0000000140d0a539  488d9768c10100                   lea rdx, [rdi + 0x1c168]
0000000140d0a540  4c8bc5                           mov r8, rbp
0000000140d0a543  488bce                           mov rcx, rsi
0000000140d0a546  e87531c3ff                       call 0x14093d6c0
0000000140d0a54b  488d9728c20100                   lea rdx, [rdi + 0x1c228]
0000000140d0a552  4c8bc5                           mov r8, rbp
0000000140d0a555  488bce                           mov rcx, rsi
0000000140d0a558  e86331c3ff                       call 0x14093d6c0
0000000140d0a55d  488d97e8c20100                   lea rdx, [rdi + 0x1c2e8]
0000000140d0a564  4c8bc5                           mov r8, rbp
0000000140d0a567  488bce                           mov rcx, rsi
0000000140d0a56a  e85131c3ff                       call 0x14093d6c0
0000000140d0a56f  488d97a8c30100                   lea rdx, [rdi + 0x1c3a8]
0000000140d0a576  4c8bc5                           mov r8, rbp
0000000140d0a579  488bce                           mov rcx, rsi
0000000140d0a57c  e83f31c3ff                       call 0x14093d6c0
0000000140d0a581  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0a588  4c8bc5                           mov r8, rbp
0000000140d0a58b  488bce                           mov rcx, rsi
0000000140d0a58e  e85d57feff                       call 0x140cefcf0
0000000140d0a593  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0a59a  4c8bc5                           mov r8, rbp
0000000140d0a59d  488bce                           mov rcx, rsi
0000000140d0a5a0  e82b65feff                       call 0x140cf0ad0
0000000140d0a5a5  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0a5ac  4c8bc5                           mov r8, rbp
0000000140d0a5af  488bce                           mov rcx, rsi
0000000140d0a5b2  e80968feff                       call 0x140cf0dc0
0000000140d0a5b7  e95b1e0000                       jmp 0x140d0c417
0000000140d0a5bc  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0a5c3  4c8bc5                           mov r8, rbp
0000000140d0a5c6  488bce                           mov rcx, rsi
0000000140d0a5c9  e8f25dfeff                       call 0x140cf03c0
0000000140d0a5ce  488bce                           mov rcx, rsi
0000000140d0a5d1  e8eadad601                       call 0x142a780c0
0000000140d0a5d6  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0a5dc  488bce                           mov rcx, rsi
0000000140d0a5df  e8dcdad601                       call 0x142a780c0
0000000140d0a5e4  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0a5ea  488bce                           mov rcx, rsi
0000000140d0a5ed  e87edcd601                       call 0x142a78270
0000000140d0a5f2  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0a5f8  488bce                           mov rcx, rsi
0000000140d0a5fb  e8c0dad601                       call 0x142a780c0
0000000140d0a600  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0a606  488bce                           mov rcx, rsi
0000000140d0a609  e862dcd601                       call 0x142a78270
0000000140d0a60e  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0a614  488bce                           mov rcx, rsi
0000000140d0a617  e854dcd601                       call 0x142a78270
0000000140d0a61c  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0a622  488bce                           mov rcx, rsi
0000000140d0a625  e896dad601                       call 0x142a780c0
0000000140d0a62a  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0a630  488bce                           mov rcx, rsi
0000000140d0a633  e838dcd601                       call 0x142a78270
0000000140d0a638  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0a63e  488bce                           mov rcx, rsi
0000000140d0a641  e87adad601                       call 0x142a780c0
0000000140d0a646  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0a64c  488bce                           mov rcx, rsi
0000000140d0a64f  e86cdad601                       call 0x142a780c0
0000000140d0a654  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0a65a  488bce                           mov rcx, rsi
0000000140d0a65d  e85edad601                       call 0x142a780c0
0000000140d0a662  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0a668  488bce                           mov rcx, rsi
0000000140d0a66b  e800dcd601                       call 0x142a78270
0000000140d0a670  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0a676  488bce                           mov rcx, rsi
0000000140d0a679  e842dad601                       call 0x142a780c0
0000000140d0a67e  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0a684  488bce                           mov rcx, rsi
0000000140d0a687  e834dad601                       call 0x142a780c0
0000000140d0a68c  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0a692  488bce                           mov rcx, rsi
0000000140d0a695  e826dad601                       call 0x142a780c0
0000000140d0a69a  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0a6a0  488bce                           mov rcx, rsi
0000000140d0a6a3  e818dad601                       call 0x142a780c0
0000000140d0a6a8  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0a6ae  488bce                           mov rcx, rsi
0000000140d0a6b1  e80adad601                       call 0x142a780c0
0000000140d0a6b6  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0a6bc  488bce                           mov rcx, rsi
0000000140d0a6bf  e8fcd9d601                       call 0x142a780c0
0000000140d0a6c4  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0a6ca  488bce                           mov rcx, rsi
0000000140d0a6cd  e8eed9d601                       call 0x142a780c0
0000000140d0a6d2  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0a6d8  488bce                           mov rcx, rsi
0000000140d0a6db  e8e0d9d601                       call 0x142a780c0
0000000140d0a6e0  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0a6e6  488bce                           mov rcx, rsi
0000000140d0a6e9  e8d2d9d601                       call 0x142a780c0
0000000140d0a6ee  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0a6f4  488bce                           mov rcx, rsi
0000000140d0a6f7  e8c4d9d601                       call 0x142a780c0
0000000140d0a6fc  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0a702  488bce                           mov rcx, rsi
0000000140d0a705  e8b6d9d601                       call 0x142a780c0
0000000140d0a70a  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0a710  488bce                           mov rcx, rsi
0000000140d0a713  e8a8d9d601                       call 0x142a780c0
0000000140d0a718  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0a71e  488bce                           mov rcx, rsi
0000000140d0a721  e89ad9d601                       call 0x142a780c0
0000000140d0a726  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0a72c  488bce                           mov rcx, rsi
0000000140d0a72f  e83cdbd601                       call 0x142a78270
0000000140d0a734  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0a73a  488bce                           mov rcx, rsi
0000000140d0a73d  e82edbd601                       call 0x142a78270
0000000140d0a742  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0a748  488bce                           mov rcx, rsi
0000000140d0a74b  e820dbd601                       call 0x142a78270
0000000140d0a750  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0a756  488bce                           mov rcx, rsi
0000000140d0a759  e862d9d601                       call 0x142a780c0
0000000140d0a75e  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0a764  488bce                           mov rcx, rsi
0000000140d0a767  e854d9d601                       call 0x142a780c0
0000000140d0a76c  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0a772  488bce                           mov rcx, rsi
0000000140d0a775  e8a6dad601                       call 0x142a78220
0000000140d0a77a  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0a781  488bce                           mov rcx, rsi
0000000140d0a784  e837d9d601                       call 0x142a780c0
0000000140d0a789  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0a78f  488d97b00d0100                   lea rdx, [rdi + 0x10db0]
0000000140d0a796  4c8bc5                           mov r8, rbp
0000000140d0a799  488bce                           mov rcx, rsi
0000000140d0a79c  e82f0fffff                       call 0x140cfb6d0
0000000140d0a7a1  4c8bc5                           mov r8, rbp
0000000140d0a7a4  488d97a8c00100                   lea rdx, [rdi + 0x1c0a8]
0000000140d0a7ab  488bce                           mov rcx, rsi
0000000140d0a7ae  e80d2fc3ff                       call 0x14093d6c0
0000000140d0a7b3  488d9768c10100                   lea rdx, [rdi + 0x1c168]
0000000140d0a7ba  4c8bc5                           mov r8, rbp
0000000140d0a7bd  488bce                           mov rcx, rsi
0000000140d0a7c0  e8fb2ec3ff                       call 0x14093d6c0
0000000140d0a7c5  488d9728c20100                   lea rdx, [rdi + 0x1c228]
0000000140d0a7cc  4c8bc5                           mov r8, rbp
0000000140d0a7cf  488bce                           mov rcx, rsi
0000000140d0a7d2  e8e92ec3ff                       call 0x14093d6c0
0000000140d0a7d7  488d97e8c20100                   lea rdx, [rdi + 0x1c2e8]
0000000140d0a7de  4c8bc5                           mov r8, rbp
0000000140d0a7e1  488bce                           mov rcx, rsi
0000000140d0a7e4  e8d72ec3ff                       call 0x14093d6c0
0000000140d0a7e9  488d97a8c30100                   lea rdx, [rdi + 0x1c3a8]
0000000140d0a7f0  4c8bc5                           mov r8, rbp
0000000140d0a7f3  488bce                           mov rcx, rsi
0000000140d0a7f6  e8c52ec3ff                       call 0x14093d6c0
0000000140d0a7fb  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0a802  4c8bc5                           mov r8, rbp
0000000140d0a805  488bce                           mov rcx, rsi
0000000140d0a808  e8e354feff                       call 0x140cefcf0
0000000140d0a80d  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0a814  4c8bc5                           mov r8, rbp
0000000140d0a817  488bce                           mov rcx, rsi
0000000140d0a81a  e8b162feff                       call 0x140cf0ad0
0000000140d0a81f  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0a826  4c8bc5                           mov r8, rbp
0000000140d0a829  488bce                           mov rcx, rsi
0000000140d0a82c  e88f65feff                       call 0x140cf0dc0
0000000140d0a831  488bce                           mov rcx, rsi
0000000140d0a834  e887d8d601                       call 0x142a780c0
0000000140d0a839  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0a83f  488bce                           mov rcx, rsi
0000000140d0a842  e829dad601                       call 0x142a78270
0000000140d0a847  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0a84d  488bce                           mov rcx, rsi
0000000140d0a850  e81bdad601                       call 0x142a78270
0000000140d0a855  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0a85b  488bce                           mov rcx, rsi
0000000140d0a85e  e85dd8d601                       call 0x142a780c0
0000000140d0a863  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0a869  488bce                           mov rcx, rsi
0000000140d0a86c  e84fd8d601                       call 0x142a780c0
0000000140d0a871  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0a877  488bce                           mov rcx, rsi
0000000140d0a87a  e841d8d601                       call 0x142a780c0
0000000140d0a87f  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0a885  e98d1b0000                       jmp 0x140d0c417
0000000140d0a88a  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0a891  4c8bc5                           mov r8, rbp
0000000140d0a894  488bce                           mov rcx, rsi
0000000140d0a897  e8245bfeff                       call 0x140cf03c0
0000000140d0a89c  488bce                           mov rcx, rsi
0000000140d0a89f  e81cd8d601                       call 0x142a780c0
0000000140d0a8a4  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0a8aa  488bce                           mov rcx, rsi
0000000140d0a8ad  e80ed8d601                       call 0x142a780c0
0000000140d0a8b2  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0a8b8  488bce                           mov rcx, rsi
0000000140d0a8bb  e8b0d9d601                       call 0x142a78270
0000000140d0a8c0  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0a8c6  488bce                           mov rcx, rsi
0000000140d0a8c9  e8f2d7d601                       call 0x142a780c0
0000000140d0a8ce  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0a8d4  488bce                           mov rcx, rsi
0000000140d0a8d7  e894d9d601                       call 0x142a78270
0000000140d0a8dc  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0a8e2  488bce                           mov rcx, rsi
0000000140d0a8e5  e886d9d601                       call 0x142a78270
0000000140d0a8ea  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0a8f0  488bce                           mov rcx, rsi
0000000140d0a8f3  e8c8d7d601                       call 0x142a780c0
0000000140d0a8f8  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0a8fe  488bce                           mov rcx, rsi
0000000140d0a901  e86ad9d601                       call 0x142a78270
0000000140d0a906  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0a90c  488bce                           mov rcx, rsi
0000000140d0a90f  e8acd7d601                       call 0x142a780c0
0000000140d0a914  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0a91a  488bce                           mov rcx, rsi
0000000140d0a91d  e89ed7d601                       call 0x142a780c0
0000000140d0a922  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0a928  488bce                           mov rcx, rsi
0000000140d0a92b  e890d7d601                       call 0x142a780c0
0000000140d0a930  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0a936  488bce                           mov rcx, rsi
0000000140d0a939  e832d9d601                       call 0x142a78270
0000000140d0a93e  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0a944  488bce                           mov rcx, rsi
0000000140d0a947  e874d7d601                       call 0x142a780c0
0000000140d0a94c  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0a952  488bce                           mov rcx, rsi
0000000140d0a955  e866d7d601                       call 0x142a780c0
0000000140d0a95a  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0a960  488bce                           mov rcx, rsi
0000000140d0a963  e858d7d601                       call 0x142a780c0
0000000140d0a968  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0a96e  488bce                           mov rcx, rsi
0000000140d0a971  e84ad7d601                       call 0x142a780c0
0000000140d0a976  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0a97c  488bce                           mov rcx, rsi
0000000140d0a97f  e83cd7d601                       call 0x142a780c0
0000000140d0a984  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0a98a  488bce                           mov rcx, rsi
0000000140d0a98d  e82ed7d601                       call 0x142a780c0
0000000140d0a992  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0a998  488bce                           mov rcx, rsi
0000000140d0a99b  e820d7d601                       call 0x142a780c0
0000000140d0a9a0  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0a9a6  488bce                           mov rcx, rsi
0000000140d0a9a9  e812d7d601                       call 0x142a780c0
0000000140d0a9ae  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0a9b4  488bce                           mov rcx, rsi
0000000140d0a9b7  e804d7d601                       call 0x142a780c0
0000000140d0a9bc  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0a9c2  488bce                           mov rcx, rsi
0000000140d0a9c5  e8f6d6d601                       call 0x142a780c0
0000000140d0a9ca  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0a9d0  488bce                           mov rcx, rsi
0000000140d0a9d3  e8e8d6d601                       call 0x142a780c0
0000000140d0a9d8  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0a9de  488bce                           mov rcx, rsi
0000000140d0a9e1  e8dad6d601                       call 0x142a780c0
0000000140d0a9e6  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0a9ec  488bce                           mov rcx, rsi
0000000140d0a9ef  e8ccd6d601                       call 0x142a780c0
0000000140d0a9f4  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0a9fa  488bce                           mov rcx, rsi
0000000140d0a9fd  e86ed8d601                       call 0x142a78270
0000000140d0aa02  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0aa08  488bce                           mov rcx, rsi
0000000140d0aa0b  e860d8d601                       call 0x142a78270
0000000140d0aa10  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0aa16  488bce                           mov rcx, rsi
0000000140d0aa19  e852d8d601                       call 0x142a78270
0000000140d0aa1e  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0aa24  488bce                           mov rcx, rsi
0000000140d0aa27  e894d6d601                       call 0x142a780c0
0000000140d0aa2c  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0aa32  488bce                           mov rcx, rsi
0000000140d0aa35  e886d6d601                       call 0x142a780c0
0000000140d0aa3a  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0aa40  488bce                           mov rcx, rsi
0000000140d0aa43  e8d8d7d601                       call 0x142a78220
0000000140d0aa48  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0aa4f  488bce                           mov rcx, rsi
0000000140d0aa52  e869d6d601                       call 0x142a780c0
0000000140d0aa57  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0aa5d  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0aa64  4c8bc5                           mov r8, rbp
0000000140d0aa67  488bce                           mov rcx, rsi
0000000140d0aa6a  e86160feff                       call 0x140cf0ad0
0000000140d0aa6f  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0aa76  4c8bc5                           mov r8, rbp
0000000140d0aa79  488bce                           mov rcx, rsi
0000000140d0aa7c  e83f63feff                       call 0x140cf0dc0
0000000140d0aa81  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0aa88  4c8bc5                           mov r8, rbp
0000000140d0aa8b  488bce                           mov rcx, rsi
0000000140d0aa8e  e85d52feff                       call 0x140cefcf0
0000000140d0aa93  e97f190000                       jmp 0x140d0c417
0000000140d0aa98  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0aa9f  4c8bc5                           mov r8, rbp
0000000140d0aaa2  488bce                           mov rcx, rsi
0000000140d0aaa5  e81659feff                       call 0x140cf03c0
0000000140d0aaaa  488bce                           mov rcx, rsi
0000000140d0aaad  e80ed6d601                       call 0x142a780c0
0000000140d0aab2  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0aab8  488bce                           mov rcx, rsi
0000000140d0aabb  e800d6d601                       call 0x142a780c0
0000000140d0aac0  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0aac6  488bce                           mov rcx, rsi
0000000140d0aac9  e8a2d7d601                       call 0x142a78270
0000000140d0aace  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0aad4  488bce                           mov rcx, rsi
0000000140d0aad7  e8e4d5d601                       call 0x142a780c0
0000000140d0aadc  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0aae2  488bce                           mov rcx, rsi
0000000140d0aae5  e886d7d601                       call 0x142a78270
0000000140d0aaea  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0aaf0  488bce                           mov rcx, rsi
0000000140d0aaf3  e878d7d601                       call 0x142a78270
0000000140d0aaf8  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0aafe  488bce                           mov rcx, rsi
0000000140d0ab01  e8bad5d601                       call 0x142a780c0
0000000140d0ab06  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0ab0c  488bce                           mov rcx, rsi
0000000140d0ab0f  e85cd7d601                       call 0x142a78270
0000000140d0ab14  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0ab1a  488bce                           mov rcx, rsi
0000000140d0ab1d  e89ed5d601                       call 0x142a780c0
0000000140d0ab22  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0ab28  488bce                           mov rcx, rsi
0000000140d0ab2b  e890d5d601                       call 0x142a780c0
0000000140d0ab30  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0ab36  488bce                           mov rcx, rsi
0000000140d0ab39  e882d5d601                       call 0x142a780c0
0000000140d0ab3e  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0ab44  488bce                           mov rcx, rsi
0000000140d0ab47  e824d7d601                       call 0x142a78270
0000000140d0ab4c  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0ab52  488bce                           mov rcx, rsi
0000000140d0ab55  e866d5d601                       call 0x142a780c0
0000000140d0ab5a  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0ab60  488bce                           mov rcx, rsi
0000000140d0ab63  e858d5d601                       call 0x142a780c0
0000000140d0ab68  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0ab6e  488bce                           mov rcx, rsi
0000000140d0ab71  e84ad5d601                       call 0x142a780c0
0000000140d0ab76  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0ab7c  488bce                           mov rcx, rsi
0000000140d0ab7f  e83cd5d601                       call 0x142a780c0
0000000140d0ab84  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0ab8a  488bce                           mov rcx, rsi
0000000140d0ab8d  e82ed5d601                       call 0x142a780c0
0000000140d0ab92  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0ab98  488bce                           mov rcx, rsi
0000000140d0ab9b  e820d5d601                       call 0x142a780c0
0000000140d0aba0  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0aba6  488bce                           mov rcx, rsi
0000000140d0aba9  e812d5d601                       call 0x142a780c0
0000000140d0abae  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0abb4  488bce                           mov rcx, rsi
0000000140d0abb7  e804d5d601                       call 0x142a780c0
0000000140d0abbc  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0abc2  488bce                           mov rcx, rsi
0000000140d0abc5  e8f6d4d601                       call 0x142a780c0
0000000140d0abca  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0abd0  488bce                           mov rcx, rsi
0000000140d0abd3  e8e8d4d601                       call 0x142a780c0
0000000140d0abd8  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0abde  488bce                           mov rcx, rsi
0000000140d0abe1  e8dad4d601                       call 0x142a780c0
0000000140d0abe6  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0abec  488bce                           mov rcx, rsi
0000000140d0abef  e8ccd4d601                       call 0x142a780c0
0000000140d0abf4  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0abfa  488bce                           mov rcx, rsi
0000000140d0abfd  e8bed4d601                       call 0x142a780c0
0000000140d0ac02  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0ac08  488bce                           mov rcx, rsi
0000000140d0ac0b  e860d6d601                       call 0x142a78270
0000000140d0ac10  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0ac16  488bce                           mov rcx, rsi
0000000140d0ac19  e852d6d601                       call 0x142a78270
0000000140d0ac1e  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0ac24  488bce                           mov rcx, rsi
0000000140d0ac27  e844d6d601                       call 0x142a78270
0000000140d0ac2c  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0ac32  488bce                           mov rcx, rsi
0000000140d0ac35  e886d4d601                       call 0x142a780c0
0000000140d0ac3a  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0ac40  488bce                           mov rcx, rsi
0000000140d0ac43  e878d4d601                       call 0x142a780c0
0000000140d0ac48  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0ac4e  488bce                           mov rcx, rsi
0000000140d0ac51  e8cad5d601                       call 0x142a78220
0000000140d0ac56  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0ac5d  488bce                           mov rcx, rsi
0000000140d0ac60  e85bd4d601                       call 0x142a780c0
0000000140d0ac65  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0ac6b  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0ac72  4c8bc5                           mov r8, rbp
0000000140d0ac75  488bce                           mov rcx, rsi
0000000140d0ac78  e8535efeff                       call 0x140cf0ad0
0000000140d0ac7d  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0ac84  4c8bc5                           mov r8, rbp
0000000140d0ac87  488bce                           mov rcx, rsi
0000000140d0ac8a  e83161feff                       call 0x140cf0dc0
0000000140d0ac8f  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0ac96  4c8bc5                           mov r8, rbp
0000000140d0ac99  488bce                           mov rcx, rsi
0000000140d0ac9c  e84f50feff                       call 0x140cefcf0
0000000140d0aca1  e98bfbffff                       jmp 0x140d0a831
0000000140d0aca6  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0acad  4c8bc5                           mov r8, rbp
0000000140d0acb0  488bce                           mov rcx, rsi
0000000140d0acb3  e80857feff                       call 0x140cf03c0
0000000140d0acb8  488bce                           mov rcx, rsi
0000000140d0acbb  e800d4d601                       call 0x142a780c0
0000000140d0acc0  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0acc6  488bce                           mov rcx, rsi
0000000140d0acc9  e8f2d3d601                       call 0x142a780c0
0000000140d0acce  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0acd4  488bce                           mov rcx, rsi
0000000140d0acd7  e894d5d601                       call 0x142a78270
0000000140d0acdc  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0ace2  488bce                           mov rcx, rsi
0000000140d0ace5  e8d6d3d601                       call 0x142a780c0
0000000140d0acea  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0acf0  488bce                           mov rcx, rsi
0000000140d0acf3  e878d5d601                       call 0x142a78270
0000000140d0acf8  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0acfe  488bce                           mov rcx, rsi
0000000140d0ad01  e86ad5d601                       call 0x142a78270
0000000140d0ad06  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0ad0c  488bce                           mov rcx, rsi
0000000140d0ad0f  e8acd3d601                       call 0x142a780c0
0000000140d0ad14  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0ad1a  488bce                           mov rcx, rsi
0000000140d0ad1d  e84ed5d601                       call 0x142a78270
0000000140d0ad22  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0ad28  488bce                           mov rcx, rsi
0000000140d0ad2b  e890d3d601                       call 0x142a780c0
0000000140d0ad30  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0ad36  488bce                           mov rcx, rsi
0000000140d0ad39  e882d3d601                       call 0x142a780c0
0000000140d0ad3e  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0ad44  488bce                           mov rcx, rsi
0000000140d0ad47  e874d3d601                       call 0x142a780c0
0000000140d0ad4c  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0ad52  488bce                           mov rcx, rsi
0000000140d0ad55  e816d5d601                       call 0x142a78270
0000000140d0ad5a  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0ad60  488bce                           mov rcx, rsi
0000000140d0ad63  e858d3d601                       call 0x142a780c0
0000000140d0ad68  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0ad6e  488bce                           mov rcx, rsi
0000000140d0ad71  e84ad3d601                       call 0x142a780c0
0000000140d0ad76  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0ad7c  488bce                           mov rcx, rsi
0000000140d0ad7f  e83cd3d601                       call 0x142a780c0
0000000140d0ad84  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0ad8a  488bce                           mov rcx, rsi
0000000140d0ad8d  e82ed3d601                       call 0x142a780c0
0000000140d0ad92  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0ad98  488bce                           mov rcx, rsi
0000000140d0ad9b  e820d3d601                       call 0x142a780c0
0000000140d0ada0  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0ada6  488bce                           mov rcx, rsi
0000000140d0ada9  e812d3d601                       call 0x142a780c0
0000000140d0adae  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0adb4  488bce                           mov rcx, rsi
0000000140d0adb7  e804d3d601                       call 0x142a780c0
0000000140d0adbc  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0adc2  488bce                           mov rcx, rsi
0000000140d0adc5  e8f6d2d601                       call 0x142a780c0
0000000140d0adca  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0add0  488bce                           mov rcx, rsi
0000000140d0add3  e8e8d2d601                       call 0x142a780c0
0000000140d0add8  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0adde  488bce                           mov rcx, rsi
0000000140d0ade1  e8dad2d601                       call 0x142a780c0
0000000140d0ade6  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0adec  488bce                           mov rcx, rsi
0000000140d0adef  e8ccd2d601                       call 0x142a780c0
0000000140d0adf4  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0adfa  488bce                           mov rcx, rsi
0000000140d0adfd  e8bed2d601                       call 0x142a780c0
0000000140d0ae02  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0ae08  488bce                           mov rcx, rsi
0000000140d0ae0b  e8b0d2d601                       call 0x142a780c0
0000000140d0ae10  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0ae16  488bce                           mov rcx, rsi
0000000140d0ae19  e852d4d601                       call 0x142a78270
0000000140d0ae1e  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0ae24  488bce                           mov rcx, rsi
0000000140d0ae27  e844d4d601                       call 0x142a78270
0000000140d0ae2c  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0ae32  488bce                           mov rcx, rsi
0000000140d0ae35  e836d4d601                       call 0x142a78270
0000000140d0ae3a  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0ae40  488bce                           mov rcx, rsi
0000000140d0ae43  e878d2d601                       call 0x142a780c0
0000000140d0ae48  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0ae4e  488bce                           mov rcx, rsi
0000000140d0ae51  e86ad2d601                       call 0x142a780c0
0000000140d0ae56  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0ae5c  488bce                           mov rcx, rsi
0000000140d0ae5f  e8bcd3d601                       call 0x142a78220
0000000140d0ae64  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0ae6b  488bce                           mov rcx, rsi
0000000140d0ae6e  e84dd2d601                       call 0x142a780c0
0000000140d0ae73  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0ae79  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0ae80  4c8bc5                           mov r8, rbp
0000000140d0ae83  488bce                           mov rcx, rsi
0000000140d0ae86  e8455cfeff                       call 0x140cf0ad0
0000000140d0ae8b  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0ae92  4c8bc5                           mov r8, rbp
0000000140d0ae95  488bce                           mov rcx, rsi
0000000140d0ae98  e8235ffeff                       call 0x140cf0dc0
0000000140d0ae9d  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0aea4  4c8bc5                           mov r8, rbp
0000000140d0aea7  488bce                           mov rcx, rsi
0000000140d0aeaa  e8414efeff                       call 0x140cefcf0
0000000140d0aeaf  488bce                           mov rcx, rsi
0000000140d0aeb2  e809d2d601                       call 0x142a780c0
0000000140d0aeb7  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0aebd  488bce                           mov rcx, rsi
0000000140d0aec0  e8abd3d601                       call 0x142a78270
0000000140d0aec5  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0aecb  488bce                           mov rcx, rsi
0000000140d0aece  e89dd3d601                       call 0x142a78270
0000000140d0aed3  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0aed9  488bce                           mov rcx, rsi
0000000140d0aedc  e8dfd1d601                       call 0x142a780c0
0000000140d0aee1  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0aee7  488bce                           mov rcx, rsi
0000000140d0aeea  e8d1d1d601                       call 0x142a780c0
0000000140d0aeef  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0aef5  488bce                           mov rcx, rsi
0000000140d0aef8  e8c3d1d601                       call 0x142a780c0
0000000140d0aefd  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0af03  488bce                           mov rcx, rsi
0000000140d0af06  e865d3d601                       call 0x142a78270
0000000140d0af0b  448bf0                           mov r14d, eax
0000000140d0af0e  488bce                           mov rcx, rsi
0000000140d0af11  e85ad3d601                       call 0x142a78270
0000000140d0af16  448bf8                           mov r15d, eax
0000000140d0af19  e9f9140000                       jmp 0x140d0c417
0000000140d0af1e  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0af25  4c8bc5                           mov r8, rbp
0000000140d0af28  488bce                           mov rcx, rsi
0000000140d0af2b  e89054feff                       call 0x140cf03c0
0000000140d0af30  488bce                           mov rcx, rsi
0000000140d0af33  e888d1d601                       call 0x142a780c0
0000000140d0af38  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0af3e  488bce                           mov rcx, rsi
0000000140d0af41  e87ad1d601                       call 0x142a780c0
0000000140d0af46  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0af4c  488bce                           mov rcx, rsi
0000000140d0af4f  e81cd3d601                       call 0x142a78270
0000000140d0af54  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0af5a  488bce                           mov rcx, rsi
0000000140d0af5d  e85ed1d601                       call 0x142a780c0
0000000140d0af62  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0af68  488bce                           mov rcx, rsi
0000000140d0af6b  e800d3d601                       call 0x142a78270
0000000140d0af70  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0af76  488bce                           mov rcx, rsi
0000000140d0af79  e8f2d2d601                       call 0x142a78270
0000000140d0af7e  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0af84  488bce                           mov rcx, rsi
0000000140d0af87  e834d1d601                       call 0x142a780c0
0000000140d0af8c  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0af92  488bce                           mov rcx, rsi
0000000140d0af95  e8d6d2d601                       call 0x142a78270
0000000140d0af9a  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0afa0  488bce                           mov rcx, rsi
0000000140d0afa3  e818d1d601                       call 0x142a780c0
0000000140d0afa8  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0afae  488bce                           mov rcx, rsi
0000000140d0afb1  e80ad1d601                       call 0x142a780c0
0000000140d0afb6  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0afbc  488bce                           mov rcx, rsi
0000000140d0afbf  e8fcd0d601                       call 0x142a780c0
0000000140d0afc4  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0afca  488bce                           mov rcx, rsi
0000000140d0afcd  e89ed2d601                       call 0x142a78270
0000000140d0afd2  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0afd8  488bce                           mov rcx, rsi
0000000140d0afdb  e8e0d0d601                       call 0x142a780c0
0000000140d0afe0  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0afe6  488bce                           mov rcx, rsi
0000000140d0afe9  e8d2d0d601                       call 0x142a780c0
0000000140d0afee  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0aff4  488bce                           mov rcx, rsi
0000000140d0aff7  e8c4d0d601                       call 0x142a780c0
0000000140d0affc  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0b002  488bce                           mov rcx, rsi
0000000140d0b005  e8b6d0d601                       call 0x142a780c0
0000000140d0b00a  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0b010  488bce                           mov rcx, rsi
0000000140d0b013  e8a8d0d601                       call 0x142a780c0
0000000140d0b018  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0b01e  488bce                           mov rcx, rsi
0000000140d0b021  e89ad0d601                       call 0x142a780c0
0000000140d0b026  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0b02c  488bce                           mov rcx, rsi
0000000140d0b02f  e88cd0d601                       call 0x142a780c0
0000000140d0b034  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0b03a  488bce                           mov rcx, rsi
0000000140d0b03d  e87ed0d601                       call 0x142a780c0
0000000140d0b042  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0b048  488bce                           mov rcx, rsi
0000000140d0b04b  e870d0d601                       call 0x142a780c0
0000000140d0b050  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0b056  488bce                           mov rcx, rsi
0000000140d0b059  e862d0d601                       call 0x142a780c0
0000000140d0b05e  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0b064  488bce                           mov rcx, rsi
0000000140d0b067  e854d0d601                       call 0x142a780c0
0000000140d0b06c  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0b072  488bce                           mov rcx, rsi
0000000140d0b075  e846d0d601                       call 0x142a780c0
0000000140d0b07a  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0b080  488bce                           mov rcx, rsi
0000000140d0b083  e838d0d601                       call 0x142a780c0
0000000140d0b088  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0b08e  488bce                           mov rcx, rsi
0000000140d0b091  e8dad1d601                       call 0x142a78270
0000000140d0b096  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0b09c  488bce                           mov rcx, rsi
0000000140d0b09f  e8ccd1d601                       call 0x142a78270
0000000140d0b0a4  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0b0aa  488bce                           mov rcx, rsi
0000000140d0b0ad  e8bed1d601                       call 0x142a78270
0000000140d0b0b2  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0b0b8  488bce                           mov rcx, rsi
0000000140d0b0bb  e800d0d601                       call 0x142a780c0
0000000140d0b0c0  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b0c6  488bce                           mov rcx, rsi
0000000140d0b0c9  e8f2cfd601                       call 0x142a780c0
0000000140d0b0ce  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0b0d4  488bce                           mov rcx, rsi
0000000140d0b0d7  e844d1d601                       call 0x142a78220
0000000140d0b0dc  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0b0e3  488bce                           mov rcx, rsi
0000000140d0b0e6  e8d5cfd601                       call 0x142a780c0
0000000140d0b0eb  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b0f1  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0b0f8  4c8bc5                           mov r8, rbp
0000000140d0b0fb  488bce                           mov rcx, rsi
0000000140d0b0fe  e8cd59feff                       call 0x140cf0ad0
0000000140d0b103  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0b10a  4c8bc5                           mov r8, rbp
0000000140d0b10d  488bce                           mov rcx, rsi
0000000140d0b110  e8ab5cfeff                       call 0x140cf0dc0
0000000140d0b115  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0b11c  4c8bc5                           mov r8, rbp
0000000140d0b11f  488bce                           mov rcx, rsi
0000000140d0b122  e8c94bfeff                       call 0x140cefcf0
0000000140d0b127  488bce                           mov rcx, rsi
0000000140d0b12a  e891cfd601                       call 0x142a780c0
0000000140d0b12f  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0b135  488bce                           mov rcx, rsi
0000000140d0b138  e833d1d601                       call 0x142a78270
0000000140d0b13d  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0b143  488bce                           mov rcx, rsi
0000000140d0b146  e825d1d601                       call 0x142a78270
0000000140d0b14b  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0b151  488bce                           mov rcx, rsi
0000000140d0b154  e867cfd601                       call 0x142a780c0
0000000140d0b159  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0b15f  488bce                           mov rcx, rsi
0000000140d0b162  e859cfd601                       call 0x142a780c0
0000000140d0b167  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0b16d  488bce                           mov rcx, rsi
0000000140d0b170  e84bcfd601                       call 0x142a780c0
0000000140d0b175  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0b17b  488bce                           mov rcx, rsi
0000000140d0b17e  e8edd0d601                       call 0x142a78270
0000000140d0b183  898744200100                     mov dword ptr [rdi + 0x12044], eax
0000000140d0b189  e989120000                       jmp 0x140d0c417
0000000140d0b18e  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0b195  4c8bc5                           mov r8, rbp
0000000140d0b198  488bce                           mov rcx, rsi
0000000140d0b19b  e82052feff                       call 0x140cf03c0
0000000140d0b1a0  488bce                           mov rcx, rsi
0000000140d0b1a3  e818cfd601                       call 0x142a780c0
0000000140d0b1a8  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0b1ae  488bce                           mov rcx, rsi
0000000140d0b1b1  e80acfd601                       call 0x142a780c0
0000000140d0b1b6  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0b1bc  488bce                           mov rcx, rsi
0000000140d0b1bf  e8acd0d601                       call 0x142a78270
0000000140d0b1c4  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0b1ca  488bce                           mov rcx, rsi
0000000140d0b1cd  e8eeced601                       call 0x142a780c0
0000000140d0b1d2  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0b1d8  488bce                           mov rcx, rsi
0000000140d0b1db  e890d0d601                       call 0x142a78270
0000000140d0b1e0  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0b1e6  488bce                           mov rcx, rsi
0000000140d0b1e9  e882d0d601                       call 0x142a78270
0000000140d0b1ee  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0b1f4  488bce                           mov rcx, rsi
0000000140d0b1f7  e8c4ced601                       call 0x142a780c0
0000000140d0b1fc  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0b202  488bce                           mov rcx, rsi
0000000140d0b205  e866d0d601                       call 0x142a78270
0000000140d0b20a  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0b210  488bce                           mov rcx, rsi
0000000140d0b213  e8a8ced601                       call 0x142a780c0
0000000140d0b218  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0b21e  488bce                           mov rcx, rsi
0000000140d0b221  e89aced601                       call 0x142a780c0
0000000140d0b226  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0b22c  488bce                           mov rcx, rsi
0000000140d0b22f  e88cced601                       call 0x142a780c0
0000000140d0b234  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0b23a  488bce                           mov rcx, rsi
0000000140d0b23d  e82ed0d601                       call 0x142a78270
0000000140d0b242  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0b248  488bce                           mov rcx, rsi
0000000140d0b24b  e870ced601                       call 0x142a780c0
0000000140d0b250  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0b256  488bce                           mov rcx, rsi
0000000140d0b259  e862ced601                       call 0x142a780c0
0000000140d0b25e  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0b264  488bce                           mov rcx, rsi
0000000140d0b267  e854ced601                       call 0x142a780c0
0000000140d0b26c  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0b272  488bce                           mov rcx, rsi
0000000140d0b275  e846ced601                       call 0x142a780c0
0000000140d0b27a  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0b280  488bce                           mov rcx, rsi
0000000140d0b283  e838ced601                       call 0x142a780c0
0000000140d0b288  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0b28e  488bce                           mov rcx, rsi
0000000140d0b291  e82aced601                       call 0x142a780c0
0000000140d0b296  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0b29c  488bce                           mov rcx, rsi
0000000140d0b29f  e81cced601                       call 0x142a780c0
0000000140d0b2a4  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0b2aa  488bce                           mov rcx, rsi
0000000140d0b2ad  e80eced601                       call 0x142a780c0
0000000140d0b2b2  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0b2b8  488bce                           mov rcx, rsi
0000000140d0b2bb  e800ced601                       call 0x142a780c0
0000000140d0b2c0  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0b2c6  488bce                           mov rcx, rsi
0000000140d0b2c9  e8f2cdd601                       call 0x142a780c0
0000000140d0b2ce  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0b2d4  488bce                           mov rcx, rsi
0000000140d0b2d7  e8e4cdd601                       call 0x142a780c0
0000000140d0b2dc  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0b2e2  488bce                           mov rcx, rsi
0000000140d0b2e5  e8d6cdd601                       call 0x142a780c0
0000000140d0b2ea  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0b2f0  488bce                           mov rcx, rsi
0000000140d0b2f3  e8c8cdd601                       call 0x142a780c0
0000000140d0b2f8  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0b2fe  488bce                           mov rcx, rsi
0000000140d0b301  e86acfd601                       call 0x142a78270
0000000140d0b306  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0b30c  488bce                           mov rcx, rsi
0000000140d0b30f  e85ccfd601                       call 0x142a78270
0000000140d0b314  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0b31a  488bce                           mov rcx, rsi
0000000140d0b31d  e84ecfd601                       call 0x142a78270
0000000140d0b322  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0b328  488bce                           mov rcx, rsi
0000000140d0b32b  e890cdd601                       call 0x142a780c0
0000000140d0b330  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b336  488bce                           mov rcx, rsi
0000000140d0b339  e882cdd601                       call 0x142a780c0
0000000140d0b33e  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0b344  488bce                           mov rcx, rsi
0000000140d0b347  e8d4ced601                       call 0x142a78220
0000000140d0b34c  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0b353  488bce                           mov rcx, rsi
0000000140d0b356  e865cdd601                       call 0x142a780c0
0000000140d0b35b  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b361  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0b368  4c8bc5                           mov r8, rbp
0000000140d0b36b  488bce                           mov rcx, rsi
0000000140d0b36e  e85d57feff                       call 0x140cf0ad0
0000000140d0b373  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0b37a  4c8bc5                           mov r8, rbp
0000000140d0b37d  488bce                           mov rcx, rsi
0000000140d0b380  e83b5afeff                       call 0x140cf0dc0
0000000140d0b385  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0b38c  4c8bc5                           mov r8, rbp
0000000140d0b38f  488bce                           mov rcx, rsi
0000000140d0b392  e85949feff                       call 0x140cefcf0
0000000140d0b397  488bce                           mov rcx, rsi
0000000140d0b39a  e821cdd601                       call 0x142a780c0
0000000140d0b39f  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0b3a5  488bce                           mov rcx, rsi
0000000140d0b3a8  e8c3ced601                       call 0x142a78270
0000000140d0b3ad  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0b3b3  488bce                           mov rcx, rsi
0000000140d0b3b6  e8b5ced601                       call 0x142a78270
0000000140d0b3bb  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0b3c1  488bce                           mov rcx, rsi
0000000140d0b3c4  e8f7ccd601                       call 0x142a780c0
0000000140d0b3c9  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0b3cf  488bce                           mov rcx, rsi
0000000140d0b3d2  e8e9ccd601                       call 0x142a780c0
0000000140d0b3d7  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0b3dd  488bce                           mov rcx, rsi
0000000140d0b3e0  e8dbccd601                       call 0x142a780c0
0000000140d0b3e5  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0b3eb  488bce                           mov rcx, rsi
0000000140d0b3ee  e87dced601                       call 0x142a78270
0000000140d0b3f3  448bf0                           mov r14d, eax
0000000140d0b3f6  488bce                           mov rcx, rsi
0000000140d0b3f9  e872ced601                       call 0x142a78270
0000000140d0b3fe  448bf8                           mov r15d, eax
0000000140d0b401  488bce                           mov rcx, rsi
0000000140d0b404  e867ced601                       call 0x142a78270
0000000140d0b409  898744200100                     mov dword ptr [rdi + 0x12044], eax
0000000140d0b40f  e903100000                       jmp 0x140d0c417
0000000140d0b414  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0b41b  4c8bc5                           mov r8, rbp
0000000140d0b41e  488bce                           mov rcx, rsi
0000000140d0b421  e89a4ffeff                       call 0x140cf03c0
0000000140d0b426  488bce                           mov rcx, rsi
0000000140d0b429  e892ccd601                       call 0x142a780c0
0000000140d0b42e  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0b434  488bce                           mov rcx, rsi
0000000140d0b437  e884ccd601                       call 0x142a780c0
0000000140d0b43c  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0b442  488bce                           mov rcx, rsi
0000000140d0b445  e826ced601                       call 0x142a78270
0000000140d0b44a  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0b450  488bce                           mov rcx, rsi
0000000140d0b453  e868ccd601                       call 0x142a780c0
0000000140d0b458  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0b45e  488bce                           mov rcx, rsi
0000000140d0b461  e80aced601                       call 0x142a78270
0000000140d0b466  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0b46c  488bce                           mov rcx, rsi
0000000140d0b46f  e8fccdd601                       call 0x142a78270
0000000140d0b474  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0b47a  488bce                           mov rcx, rsi
0000000140d0b47d  e83eccd601                       call 0x142a780c0
0000000140d0b482  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0b488  488bce                           mov rcx, rsi
0000000140d0b48b  e8e0cdd601                       call 0x142a78270
0000000140d0b490  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0b496  488bce                           mov rcx, rsi
0000000140d0b499  e822ccd601                       call 0x142a780c0
0000000140d0b49e  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0b4a4  488bce                           mov rcx, rsi
0000000140d0b4a7  e814ccd601                       call 0x142a780c0
0000000140d0b4ac  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0b4b2  488bce                           mov rcx, rsi
0000000140d0b4b5  e806ccd601                       call 0x142a780c0
0000000140d0b4ba  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0b4c0  488bce                           mov rcx, rsi
0000000140d0b4c3  e8a8cdd601                       call 0x142a78270
0000000140d0b4c8  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0b4ce  488bce                           mov rcx, rsi
0000000140d0b4d1  e8eacbd601                       call 0x142a780c0
0000000140d0b4d6  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0b4dc  488bce                           mov rcx, rsi
0000000140d0b4df  e8dccbd601                       call 0x142a780c0
0000000140d0b4e4  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0b4ea  488bce                           mov rcx, rsi
0000000140d0b4ed  e8cecbd601                       call 0x142a780c0
0000000140d0b4f2  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0b4f8  488bce                           mov rcx, rsi
0000000140d0b4fb  e8c0cbd601                       call 0x142a780c0
0000000140d0b500  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0b506  488bce                           mov rcx, rsi
0000000140d0b509  e8b2cbd601                       call 0x142a780c0
0000000140d0b50e  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0b514  488bce                           mov rcx, rsi
0000000140d0b517  e8a4cbd601                       call 0x142a780c0
0000000140d0b51c  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0b522  488bce                           mov rcx, rsi
0000000140d0b525  e896cbd601                       call 0x142a780c0
0000000140d0b52a  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0b530  488bce                           mov rcx, rsi
0000000140d0b533  e888cbd601                       call 0x142a780c0
0000000140d0b538  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0b53e  488bce                           mov rcx, rsi
0000000140d0b541  e87acbd601                       call 0x142a780c0
0000000140d0b546  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0b54c  488bce                           mov rcx, rsi
0000000140d0b54f  e86ccbd601                       call 0x142a780c0
0000000140d0b554  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0b55a  488bce                           mov rcx, rsi
0000000140d0b55d  e85ecbd601                       call 0x142a780c0
0000000140d0b562  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0b568  488bce                           mov rcx, rsi
0000000140d0b56b  e850cbd601                       call 0x142a780c0
0000000140d0b570  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0b576  488bce                           mov rcx, rsi
0000000140d0b579  e842cbd601                       call 0x142a780c0
0000000140d0b57e  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0b584  488bce                           mov rcx, rsi
0000000140d0b587  e8e4ccd601                       call 0x142a78270
0000000140d0b58c  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0b592  488bce                           mov rcx, rsi
0000000140d0b595  e8d6ccd601                       call 0x142a78270
0000000140d0b59a  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0b5a0  488bce                           mov rcx, rsi
0000000140d0b5a3  e8c8ccd601                       call 0x142a78270
0000000140d0b5a8  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0b5ae  488bce                           mov rcx, rsi
0000000140d0b5b1  e80acbd601                       call 0x142a780c0
0000000140d0b5b6  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b5bc  488bce                           mov rcx, rsi
0000000140d0b5bf  e8fccad601                       call 0x142a780c0
0000000140d0b5c4  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0b5ca  488bce                           mov rcx, rsi
0000000140d0b5cd  e84eccd601                       call 0x142a78220
0000000140d0b5d2  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0b5d9  488bce                           mov rcx, rsi
0000000140d0b5dc  e8dfcad601                       call 0x142a780c0
0000000140d0b5e1  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b5e7  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0b5ee  4c8bc5                           mov r8, rbp
0000000140d0b5f1  488bce                           mov rcx, rsi
0000000140d0b5f4  e8d754feff                       call 0x140cf0ad0
0000000140d0b5f9  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0b600  4c8bc5                           mov r8, rbp
0000000140d0b603  488bce                           mov rcx, rsi
0000000140d0b606  e8b557feff                       call 0x140cf0dc0
0000000140d0b60b  488d97d0ce0100                   lea rdx, [rdi + 0x1ced0]
0000000140d0b612  4c8bc5                           mov r8, rbp
0000000140d0b615  488bce                           mov rcx, rsi
0000000140d0b618  e8d346feff                       call 0x140cefcf0
0000000140d0b61d  488bce                           mov rcx, rsi
0000000140d0b620  e89bcad601                       call 0x142a780c0
0000000140d0b625  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0b62b  488bce                           mov rcx, rsi
0000000140d0b62e  e83dccd601                       call 0x142a78270
0000000140d0b633  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0b639  488bce                           mov rcx, rsi
0000000140d0b63c  e82fccd601                       call 0x142a78270
0000000140d0b641  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0b647  488bce                           mov rcx, rsi
0000000140d0b64a  e871cad601                       call 0x142a780c0
0000000140d0b64f  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0b655  488bce                           mov rcx, rsi
0000000140d0b658  e863cad601                       call 0x142a780c0
0000000140d0b65d  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0b663  488bce                           mov rcx, rsi
0000000140d0b666  e855cad601                       call 0x142a780c0
0000000140d0b66b  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0b671  488bce                           mov rcx, rsi
0000000140d0b674  e8f7cbd601                       call 0x142a78270
0000000140d0b679  448bf0                           mov r14d, eax
0000000140d0b67c  488bce                           mov rcx, rsi
0000000140d0b67f  e8eccbd601                       call 0x142a78270
0000000140d0b684  448bf8                           mov r15d, eax
0000000140d0b687  488bce                           mov rcx, rsi
0000000140d0b68a  e8e1cbd601                       call 0x142a78270
0000000140d0b68f  898744200100                     mov dword ptr [rdi + 0x12044], eax
0000000140d0b695  488bce                           mov rcx, rsi
0000000140d0b698  e823cad601                       call 0x142a780c0
0000000140d0b69d  8887f81e0100                     mov byte ptr [rdi + 0x11ef8], al
0000000140d0b6a3  488bce                           mov rcx, rsi
0000000140d0b6a6  e815cad601                       call 0x142a780c0
0000000140d0b6ab  8887f91e0100                     mov byte ptr [rdi + 0x11ef9], al
0000000140d0b6b1  e9610d0000                       jmp 0x140d0c417
0000000140d0b6b6  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0b6bd  4c8bc5                           mov r8, rbp
0000000140d0b6c0  488bce                           mov rcx, rsi
0000000140d0b6c3  e8f84cfeff                       call 0x140cf03c0
0000000140d0b6c8  488bce                           mov rcx, rsi
0000000140d0b6cb  e8f0c9d601                       call 0x142a780c0
0000000140d0b6d0  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0b6d6  488bce                           mov rcx, rsi
0000000140d0b6d9  e8e2c9d601                       call 0x142a780c0
0000000140d0b6de  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0b6e4  488bce                           mov rcx, rsi
0000000140d0b6e7  e884cbd601                       call 0x142a78270
0000000140d0b6ec  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0b6f2  488bce                           mov rcx, rsi
0000000140d0b6f5  e8c6c9d601                       call 0x142a780c0
0000000140d0b6fa  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0b700  488bce                           mov rcx, rsi
0000000140d0b703  e868cbd601                       call 0x142a78270
0000000140d0b708  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0b70e  488bce                           mov rcx, rsi
0000000140d0b711  e85acbd601                       call 0x142a78270
0000000140d0b716  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0b71c  488bce                           mov rcx, rsi
0000000140d0b71f  e89cc9d601                       call 0x142a780c0
0000000140d0b724  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0b72a  488bce                           mov rcx, rsi
0000000140d0b72d  e83ecbd601                       call 0x142a78270
0000000140d0b732  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0b738  488bce                           mov rcx, rsi
0000000140d0b73b  e880c9d601                       call 0x142a780c0
0000000140d0b740  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0b746  488bce                           mov rcx, rsi
0000000140d0b749  e872c9d601                       call 0x142a780c0
0000000140d0b74e  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0b754  488bce                           mov rcx, rsi
0000000140d0b757  e864c9d601                       call 0x142a780c0
0000000140d0b75c  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0b762  488bce                           mov rcx, rsi
0000000140d0b765  e806cbd601                       call 0x142a78270
0000000140d0b76a  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0b770  488bce                           mov rcx, rsi
0000000140d0b773  e848c9d601                       call 0x142a780c0
0000000140d0b778  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0b77e  488bce                           mov rcx, rsi
0000000140d0b781  e83ac9d601                       call 0x142a780c0
0000000140d0b786  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0b78c  488bce                           mov rcx, rsi
0000000140d0b78f  e82cc9d601                       call 0x142a780c0
0000000140d0b794  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0b79a  488bce                           mov rcx, rsi
0000000140d0b79d  e81ec9d601                       call 0x142a780c0
0000000140d0b7a2  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0b7a8  488bce                           mov rcx, rsi
0000000140d0b7ab  e810c9d601                       call 0x142a780c0
0000000140d0b7b0  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0b7b6  488bce                           mov rcx, rsi
0000000140d0b7b9  e802c9d601                       call 0x142a780c0
0000000140d0b7be  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0b7c4  488bce                           mov rcx, rsi
0000000140d0b7c7  e8f4c8d601                       call 0x142a780c0
0000000140d0b7cc  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0b7d2  488bce                           mov rcx, rsi
0000000140d0b7d5  e8e6c8d601                       call 0x142a780c0
0000000140d0b7da  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0b7e0  488bce                           mov rcx, rsi
0000000140d0b7e3  e8d8c8d601                       call 0x142a780c0
0000000140d0b7e8  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0b7ee  488bce                           mov rcx, rsi
0000000140d0b7f1  e8cac8d601                       call 0x142a780c0
0000000140d0b7f6  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0b7fc  488bce                           mov rcx, rsi
0000000140d0b7ff  e8bcc8d601                       call 0x142a780c0
0000000140d0b804  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0b80a  488bce                           mov rcx, rsi
0000000140d0b80d  e8aec8d601                       call 0x142a780c0
0000000140d0b812  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0b818  488bce                           mov rcx, rsi
0000000140d0b81b  e8a0c8d601                       call 0x142a780c0
0000000140d0b820  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0b826  488bce                           mov rcx, rsi
0000000140d0b829  e842cad601                       call 0x142a78270
0000000140d0b82e  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0b834  488bce                           mov rcx, rsi
0000000140d0b837  e834cad601                       call 0x142a78270
0000000140d0b83c  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0b842  488bce                           mov rcx, rsi
0000000140d0b845  e826cad601                       call 0x142a78270
0000000140d0b84a  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0b850  488bce                           mov rcx, rsi
0000000140d0b853  e868c8d601                       call 0x142a780c0
0000000140d0b858  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b85e  488bce                           mov rcx, rsi
0000000140d0b861  e85ac8d601                       call 0x142a780c0
0000000140d0b866  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0b86c  488bce                           mov rcx, rsi
0000000140d0b86f  e8acc9d601                       call 0x142a78220
0000000140d0b874  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0b87b  488bce                           mov rcx, rsi
0000000140d0b87e  e83dc8d601                       call 0x142a780c0
0000000140d0b883  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0b889  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0b890  4c8bc5                           mov r8, rbp
0000000140d0b893  488bce                           mov rcx, rsi
0000000140d0b896  e83552feff                       call 0x140cf0ad0
0000000140d0b89b  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0b8a2  4c8bc5                           mov r8, rbp
0000000140d0b8a5  488bce                           mov rcx, rsi
0000000140d0b8a8  e81355feff                       call 0x140cf0dc0
0000000140d0b8ad  488d9708cf0100                   lea rdx, [rdi + 0x1cf08]
0000000140d0b8b4  4c8bc5                           mov r8, rbp
0000000140d0b8b7  488bce                           mov rcx, rsi
0000000140d0b8ba  e8315cfeff                       call 0x140cf14f0
0000000140d0b8bf  e959fdffff                       jmp 0x140d0b61d
0000000140d0b8c4  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0b8cb  4c8bc5                           mov r8, rbp
0000000140d0b8ce  488bce                           mov rcx, rsi
0000000140d0b8d1  e8ea4afeff                       call 0x140cf03c0
0000000140d0b8d6  488bce                           mov rcx, rsi
0000000140d0b8d9  e8e2c7d601                       call 0x142a780c0
0000000140d0b8de  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0b8e4  488bce                           mov rcx, rsi
0000000140d0b8e7  e8d4c7d601                       call 0x142a780c0
0000000140d0b8ec  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0b8f2  488bce                           mov rcx, rsi
0000000140d0b8f5  e876c9d601                       call 0x142a78270
0000000140d0b8fa  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0b900  488bce                           mov rcx, rsi
0000000140d0b903  e8b8c7d601                       call 0x142a780c0
0000000140d0b908  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0b90e  488bce                           mov rcx, rsi
0000000140d0b911  e85ac9d601                       call 0x142a78270
0000000140d0b916  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0b91c  488bce                           mov rcx, rsi
0000000140d0b91f  e84cc9d601                       call 0x142a78270
0000000140d0b924  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0b92a  488bce                           mov rcx, rsi
0000000140d0b92d  e88ec7d601                       call 0x142a780c0
0000000140d0b932  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0b938  488bce                           mov rcx, rsi
0000000140d0b93b  e830c9d601                       call 0x142a78270
0000000140d0b940  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0b946  488bce                           mov rcx, rsi
0000000140d0b949  e872c7d601                       call 0x142a780c0
0000000140d0b94e  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0b954  488bce                           mov rcx, rsi
0000000140d0b957  e864c7d601                       call 0x142a780c0
0000000140d0b95c  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0b962  488bce                           mov rcx, rsi
0000000140d0b965  e856c7d601                       call 0x142a780c0
0000000140d0b96a  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0b970  488bce                           mov rcx, rsi
0000000140d0b973  e8f8c8d601                       call 0x142a78270
0000000140d0b978  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0b97e  488bce                           mov rcx, rsi
0000000140d0b981  e83ac7d601                       call 0x142a780c0
0000000140d0b986  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0b98c  488bce                           mov rcx, rsi
0000000140d0b98f  e82cc7d601                       call 0x142a780c0
0000000140d0b994  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0b99a  488bce                           mov rcx, rsi
0000000140d0b99d  e81ec7d601                       call 0x142a780c0
0000000140d0b9a2  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0b9a8  488bce                           mov rcx, rsi
0000000140d0b9ab  e810c7d601                       call 0x142a780c0
0000000140d0b9b0  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0b9b6  488bce                           mov rcx, rsi
0000000140d0b9b9  e802c7d601                       call 0x142a780c0
0000000140d0b9be  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0b9c4  488bce                           mov rcx, rsi
0000000140d0b9c7  e8f4c6d601                       call 0x142a780c0
0000000140d0b9cc  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0b9d2  488bce                           mov rcx, rsi
0000000140d0b9d5  e8e6c6d601                       call 0x142a780c0
0000000140d0b9da  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0b9e0  488bce                           mov rcx, rsi
0000000140d0b9e3  e8d8c6d601                       call 0x142a780c0
0000000140d0b9e8  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0b9ee  488bce                           mov rcx, rsi
0000000140d0b9f1  e8cac6d601                       call 0x142a780c0
0000000140d0b9f6  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0b9fc  488bce                           mov rcx, rsi
0000000140d0b9ff  e8bcc6d601                       call 0x142a780c0
0000000140d0ba04  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0ba0a  488bce                           mov rcx, rsi
0000000140d0ba0d  e8aec6d601                       call 0x142a780c0
0000000140d0ba12  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0ba18  488bce                           mov rcx, rsi
0000000140d0ba1b  e8a0c6d601                       call 0x142a780c0
0000000140d0ba20  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0ba26  488bce                           mov rcx, rsi
0000000140d0ba29  e892c6d601                       call 0x142a780c0
0000000140d0ba2e  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0ba34  488bce                           mov rcx, rsi
0000000140d0ba37  e834c8d601                       call 0x142a78270
0000000140d0ba3c  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0ba42  488bce                           mov rcx, rsi
0000000140d0ba45  e826c8d601                       call 0x142a78270
0000000140d0ba4a  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0ba50  488bce                           mov rcx, rsi
0000000140d0ba53  e818c8d601                       call 0x142a78270
0000000140d0ba58  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0ba5e  488bce                           mov rcx, rsi
0000000140d0ba61  e85ac6d601                       call 0x142a780c0
0000000140d0ba66  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0ba6c  488bce                           mov rcx, rsi
0000000140d0ba6f  e84cc6d601                       call 0x142a780c0
0000000140d0ba74  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0ba7a  488bce                           mov rcx, rsi
0000000140d0ba7d  e89ec7d601                       call 0x142a78220
0000000140d0ba82  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0ba89  488bce                           mov rcx, rsi
0000000140d0ba8c  e82fc6d601                       call 0x142a780c0
0000000140d0ba91  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0ba97  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0ba9e  4c8bc5                           mov r8, rbp
0000000140d0baa1  488bce                           mov rcx, rsi
0000000140d0baa4  e82750feff                       call 0x140cf0ad0
0000000140d0baa9  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0bab0  4c8bc5                           mov r8, rbp
0000000140d0bab3  488bce                           mov rcx, rsi
0000000140d0bab6  e80553feff                       call 0x140cf0dc0
0000000140d0babb  488d9708cf0100                   lea rdx, [rdi + 0x1cf08]
0000000140d0bac2  4c8bc5                           mov r8, rbp
0000000140d0bac5  488bce                           mov rcx, rsi
0000000140d0bac8  e8235afeff                       call 0x140cf14f0
0000000140d0bacd  488bce                           mov rcx, rsi
0000000140d0bad0  e8ebc5d601                       call 0x142a780c0
0000000140d0bad5  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0badb  488bce                           mov rcx, rsi
0000000140d0bade  e88dc7d601                       call 0x142a78270
0000000140d0bae3  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0bae9  488bce                           mov rcx, rsi
0000000140d0baec  e87fc7d601                       call 0x142a78270
0000000140d0baf1  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0baf7  488bce                           mov rcx, rsi
0000000140d0bafa  e8c1c5d601                       call 0x142a780c0
0000000140d0baff  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0bb05  488bce                           mov rcx, rsi
0000000140d0bb08  e8b3c5d601                       call 0x142a780c0
0000000140d0bb0d  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0bb13  488bce                           mov rcx, rsi
0000000140d0bb16  e8a5c5d601                       call 0x142a780c0
0000000140d0bb1b  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0bb21  488bce                           mov rcx, rsi
0000000140d0bb24  e847c7d601                       call 0x142a78270
0000000140d0bb29  448bf0                           mov r14d, eax
0000000140d0bb2c  488bce                           mov rcx, rsi
0000000140d0bb2f  e83cc7d601                       call 0x142a78270
0000000140d0bb34  448bf8                           mov r15d, eax
0000000140d0bb37  488bce                           mov rcx, rsi
0000000140d0bb3a  e831c7d601                       call 0x142a78270
0000000140d0bb3f  898744200100                     mov dword ptr [rdi + 0x12044], eax
0000000140d0bb45  488bce                           mov rcx, rsi
0000000140d0bb48  e873c5d601                       call 0x142a780c0
0000000140d0bb4d  8887f81e0100                     mov byte ptr [rdi + 0x11ef8], al
0000000140d0bb53  488bce                           mov rcx, rsi
0000000140d0bb56  e865c5d601                       call 0x142a780c0
0000000140d0bb5b  8887f91e0100                     mov byte ptr [rdi + 0x11ef9], al
0000000140d0bb61  e995080000                       jmp 0x140d0c3fb
0000000140d0bb66  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0bb6d  4c8bc5                           mov r8, rbp
0000000140d0bb70  488bce                           mov rcx, rsi
0000000140d0bb73  e84848feff                       call 0x140cf03c0
0000000140d0bb78  488bce                           mov rcx, rsi
0000000140d0bb7b  e840c5d601                       call 0x142a780c0
0000000140d0bb80  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0bb86  488bce                           mov rcx, rsi
0000000140d0bb89  e832c5d601                       call 0x142a780c0
0000000140d0bb8e  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0bb94  488bce                           mov rcx, rsi
0000000140d0bb97  e8d4c6d601                       call 0x142a78270
0000000140d0bb9c  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0bba2  488bce                           mov rcx, rsi
0000000140d0bba5  e816c5d601                       call 0x142a780c0
0000000140d0bbaa  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0bbb0  488bce                           mov rcx, rsi
0000000140d0bbb3  e8b8c6d601                       call 0x142a78270
0000000140d0bbb8  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0bbbe  488bce                           mov rcx, rsi
0000000140d0bbc1  e8aac6d601                       call 0x142a78270
0000000140d0bbc6  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0bbcc  488bce                           mov rcx, rsi
0000000140d0bbcf  e8ecc4d601                       call 0x142a780c0
0000000140d0bbd4  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0bbda  488bce                           mov rcx, rsi
0000000140d0bbdd  e88ec6d601                       call 0x142a78270
0000000140d0bbe2  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0bbe8  488bce                           mov rcx, rsi
0000000140d0bbeb  e8d0c4d601                       call 0x142a780c0
0000000140d0bbf0  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0bbf6  488bce                           mov rcx, rsi
0000000140d0bbf9  e8c2c4d601                       call 0x142a780c0
0000000140d0bbfe  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0bc04  488bce                           mov rcx, rsi
0000000140d0bc07  e8b4c4d601                       call 0x142a780c0
0000000140d0bc0c  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0bc12  488bce                           mov rcx, rsi
0000000140d0bc15  e856c6d601                       call 0x142a78270
0000000140d0bc1a  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0bc20  488bce                           mov rcx, rsi
0000000140d0bc23  e898c4d601                       call 0x142a780c0
0000000140d0bc28  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0bc2e  488bce                           mov rcx, rsi
0000000140d0bc31  e88ac4d601                       call 0x142a780c0
0000000140d0bc36  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0bc3c  488bce                           mov rcx, rsi
0000000140d0bc3f  e87cc4d601                       call 0x142a780c0
0000000140d0bc44  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0bc4a  488bce                           mov rcx, rsi
0000000140d0bc4d  e86ec4d601                       call 0x142a780c0
0000000140d0bc52  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0bc58  488bce                           mov rcx, rsi
0000000140d0bc5b  e860c4d601                       call 0x142a780c0
0000000140d0bc60  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0bc66  488bce                           mov rcx, rsi
0000000140d0bc69  e852c4d601                       call 0x142a780c0
0000000140d0bc6e  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0bc74  488bce                           mov rcx, rsi
0000000140d0bc77  e844c4d601                       call 0x142a780c0
0000000140d0bc7c  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0bc82  488bce                           mov rcx, rsi
0000000140d0bc85  e836c4d601                       call 0x142a780c0
0000000140d0bc8a  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0bc90  488bce                           mov rcx, rsi
0000000140d0bc93  e828c4d601                       call 0x142a780c0
0000000140d0bc98  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0bc9e  488bce                           mov rcx, rsi
0000000140d0bca1  e81ac4d601                       call 0x142a780c0
0000000140d0bca6  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0bcac  488bce                           mov rcx, rsi
0000000140d0bcaf  e80cc4d601                       call 0x142a780c0
0000000140d0bcb4  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0bcba  488bce                           mov rcx, rsi
0000000140d0bcbd  e8fec3d601                       call 0x142a780c0
0000000140d0bcc2  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0bcc8  488bce                           mov rcx, rsi
0000000140d0bccb  e8f0c3d601                       call 0x142a780c0
0000000140d0bcd0  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0bcd6  488bce                           mov rcx, rsi
0000000140d0bcd9  e892c5d601                       call 0x142a78270
0000000140d0bcde  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0bce4  488bce                           mov rcx, rsi
0000000140d0bce7  e884c5d601                       call 0x142a78270
0000000140d0bcec  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0bcf2  488bce                           mov rcx, rsi
0000000140d0bcf5  e876c5d601                       call 0x142a78270
0000000140d0bcfa  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0bd00  488bce                           mov rcx, rsi
0000000140d0bd03  e8b8c3d601                       call 0x142a780c0
0000000140d0bd08  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0bd0e  488bce                           mov rcx, rsi
0000000140d0bd11  e8aac3d601                       call 0x142a780c0
0000000140d0bd16  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0bd1c  488bce                           mov rcx, rsi
0000000140d0bd1f  e8fcc4d601                       call 0x142a78220
0000000140d0bd24  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0bd2b  488bce                           mov rcx, rsi
0000000140d0bd2e  e88dc3d601                       call 0x142a780c0
0000000140d0bd33  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0bd39  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0bd40  4c8bc5                           mov r8, rbp
0000000140d0bd43  488bce                           mov rcx, rsi
0000000140d0bd46  e8854dfeff                       call 0x140cf0ad0
0000000140d0bd4b  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0bd52  4c8bc5                           mov r8, rbp
0000000140d0bd55  488bce                           mov rcx, rsi
0000000140d0bd58  e86350feff                       call 0x140cf0dc0
0000000140d0bd5d  488d9708cf0100                   lea rdx, [rdi + 0x1cf08]
0000000140d0bd64  4c8bc5                           mov r8, rbp
0000000140d0bd67  488bce                           mov rcx, rsi
0000000140d0bd6a  e88157feff                       call 0x140cf14f0
0000000140d0bd6f  488bce                           mov rcx, rsi
0000000140d0bd72  e849c3d601                       call 0x142a780c0
0000000140d0bd77  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0bd7d  488bce                           mov rcx, rsi
0000000140d0bd80  e8ebc4d601                       call 0x142a78270
0000000140d0bd85  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0bd8b  488bce                           mov rcx, rsi
0000000140d0bd8e  e8ddc4d601                       call 0x142a78270
0000000140d0bd93  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0bd99  488bce                           mov rcx, rsi
0000000140d0bd9c  e81fc3d601                       call 0x142a780c0
0000000140d0bda1  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0bda7  488bce                           mov rcx, rsi
0000000140d0bdaa  e811c3d601                       call 0x142a780c0
0000000140d0bdaf  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0bdb5  488bce                           mov rcx, rsi
0000000140d0bdb8  e803c3d601                       call 0x142a780c0
0000000140d0bdbd  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0bdc3  488bce                           mov rcx, rsi
0000000140d0bdc6  e8a5c4d601                       call 0x142a78270
0000000140d0bdcb  448bf0                           mov r14d, eax
0000000140d0bdce  488bce                           mov rcx, rsi
0000000140d0bdd1  e89ac4d601                       call 0x142a78270
0000000140d0bdd6  448bf8                           mov r15d, eax
0000000140d0bdd9  488bce                           mov rcx, rsi
0000000140d0bddc  e88fc4d601                       call 0x142a78270
0000000140d0bde1  898744200100                     mov dword ptr [rdi + 0x12044], eax
0000000140d0bde7  488bce                           mov rcx, rsi
0000000140d0bdea  e8d1c2d601                       call 0x142a780c0
0000000140d0bdef  8887f81e0100                     mov byte ptr [rdi + 0x11ef8], al
0000000140d0bdf5  488bce                           mov rcx, rsi
0000000140d0bdf8  e8c3c2d601                       call 0x142a780c0
0000000140d0bdfd  8887f91e0100                     mov byte ptr [rdi + 0x11ef9], al
0000000140d0be03  488bce                           mov rcx, rsi
0000000140d0be06  e865c4d601                       call 0x142a78270
0000000140d0be0b  898720cf0100                     mov dword ptr [rdi + 0x1cf20], eax
0000000140d0be11  e901060000                       jmp 0x140d0c417
0000000140d0be16  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0be1d  4c8bc5                           mov r8, rbp
0000000140d0be20  488bce                           mov rcx, rsi
0000000140d0be23  e89845feff                       call 0x140cf03c0
0000000140d0be28  488bce                           mov rcx, rsi
0000000140d0be2b  e890c2d601                       call 0x142a780c0
0000000140d0be30  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0be36  488bce                           mov rcx, rsi
0000000140d0be39  e882c2d601                       call 0x142a780c0
0000000140d0be3e  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0be44  488bce                           mov rcx, rsi
0000000140d0be47  e824c4d601                       call 0x142a78270
0000000140d0be4c  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0be52  488bce                           mov rcx, rsi
0000000140d0be55  e866c2d601                       call 0x142a780c0
0000000140d0be5a  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0be60  488bce                           mov rcx, rsi
0000000140d0be63  e808c4d601                       call 0x142a78270
0000000140d0be68  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0be6e  488bce                           mov rcx, rsi
0000000140d0be71  e8fac3d601                       call 0x142a78270
0000000140d0be76  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0be7c  488bce                           mov rcx, rsi
0000000140d0be7f  e83cc2d601                       call 0x142a780c0
0000000140d0be84  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0be8a  488bce                           mov rcx, rsi
0000000140d0be8d  e8dec3d601                       call 0x142a78270
0000000140d0be92  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0be98  488bce                           mov rcx, rsi
0000000140d0be9b  e820c2d601                       call 0x142a780c0
0000000140d0bea0  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0bea6  488bce                           mov rcx, rsi
0000000140d0bea9  e812c2d601                       call 0x142a780c0
0000000140d0beae  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0beb4  488bce                           mov rcx, rsi
0000000140d0beb7  e804c2d601                       call 0x142a780c0
0000000140d0bebc  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0bec2  488bce                           mov rcx, rsi
0000000140d0bec5  e8a6c3d601                       call 0x142a78270
0000000140d0beca  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0bed0  488bce                           mov rcx, rsi
0000000140d0bed3  e8e8c1d601                       call 0x142a780c0
0000000140d0bed8  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0bede  488bce                           mov rcx, rsi
0000000140d0bee1  e8dac1d601                       call 0x142a780c0
0000000140d0bee6  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0beec  488bce                           mov rcx, rsi
0000000140d0beef  e8ccc1d601                       call 0x142a780c0
0000000140d0bef4  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0befa  488bce                           mov rcx, rsi
0000000140d0befd  e8bec1d601                       call 0x142a780c0
0000000140d0bf02  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0bf08  488bce                           mov rcx, rsi
0000000140d0bf0b  e8b0c1d601                       call 0x142a780c0
0000000140d0bf10  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0bf16  488bce                           mov rcx, rsi
0000000140d0bf19  e8a2c1d601                       call 0x142a780c0
0000000140d0bf1e  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0bf24  488bce                           mov rcx, rsi
0000000140d0bf27  e894c1d601                       call 0x142a780c0
0000000140d0bf2c  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0bf32  488bce                           mov rcx, rsi
0000000140d0bf35  e886c1d601                       call 0x142a780c0
0000000140d0bf3a  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0bf40  488bce                           mov rcx, rsi
0000000140d0bf43  e878c1d601                       call 0x142a780c0
0000000140d0bf48  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0bf4e  488bce                           mov rcx, rsi
0000000140d0bf51  e86ac1d601                       call 0x142a780c0
0000000140d0bf56  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0bf5c  488bce                           mov rcx, rsi
0000000140d0bf5f  e85cc1d601                       call 0x142a780c0
0000000140d0bf64  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0bf6a  488bce                           mov rcx, rsi
0000000140d0bf6d  e84ec1d601                       call 0x142a780c0
0000000140d0bf72  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0bf78  488bce                           mov rcx, rsi
0000000140d0bf7b  e840c1d601                       call 0x142a780c0
0000000140d0bf80  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0bf86  488bce                           mov rcx, rsi
0000000140d0bf89  e8e2c2d601                       call 0x142a78270
0000000140d0bf8e  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0bf94  488bce                           mov rcx, rsi
0000000140d0bf97  e8d4c2d601                       call 0x142a78270
0000000140d0bf9c  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0bfa2  488bce                           mov rcx, rsi
0000000140d0bfa5  e8c6c2d601                       call 0x142a78270
0000000140d0bfaa  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0bfb0  488bce                           mov rcx, rsi
0000000140d0bfb3  e808c1d601                       call 0x142a780c0
0000000140d0bfb8  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0bfbe  488bce                           mov rcx, rsi
0000000140d0bfc1  e8fac0d601                       call 0x142a780c0
0000000140d0bfc6  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0bfcc  488bce                           mov rcx, rsi
0000000140d0bfcf  e84cc2d601                       call 0x142a78220
0000000140d0bfd4  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0bfdb  488bce                           mov rcx, rsi
0000000140d0bfde  e8ddc0d601                       call 0x142a780c0
0000000140d0bfe3  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0bfe9  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0bff0  4c8bc5                           mov r8, rbp
0000000140d0bff3  488bce                           mov rcx, rsi
0000000140d0bff6  e8d54afeff                       call 0x140cf0ad0
0000000140d0bffb  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0c002  4c8bc5                           mov r8, rbp
0000000140d0c005  488bce                           mov rcx, rsi
0000000140d0c008  e8b34dfeff                       call 0x140cf0dc0
0000000140d0c00d  488d9708cf0100                   lea rdx, [rdi + 0x1cf08]
0000000140d0c014  4c8bc5                           mov r8, rbp
0000000140d0c017  488bce                           mov rcx, rsi
0000000140d0c01a  e8d154feff                       call 0x140cf14f0
0000000140d0c01f  488bce                           mov rcx, rsi
0000000140d0c022  e899c0d601                       call 0x142a780c0
0000000140d0c027  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0c02d  488bce                           mov rcx, rsi
0000000140d0c030  e83bc2d601                       call 0x142a78270
0000000140d0c035  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0c03b  488bce                           mov rcx, rsi
0000000140d0c03e  e82dc2d601                       call 0x142a78270
0000000140d0c043  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0c049  488bce                           mov rcx, rsi
0000000140d0c04c  e86fc0d601                       call 0x142a780c0
0000000140d0c051  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0c057  488bce                           mov rcx, rsi
0000000140d0c05a  e861c0d601                       call 0x142a780c0
0000000140d0c05f  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0c065  488bce                           mov rcx, rsi
0000000140d0c068  e853c0d601                       call 0x142a780c0
0000000140d0c06d  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0c073  488bce                           mov rcx, rsi
0000000140d0c076  e8f5c1d601                       call 0x142a78270
0000000140d0c07b  448bf0                           mov r14d, eax
0000000140d0c07e  488bce                           mov rcx, rsi
0000000140d0c081  e8eac1d601                       call 0x142a78270
0000000140d0c086  448bf8                           mov r15d, eax
0000000140d0c089  488bce                           mov rcx, rsi
0000000140d0c08c  e8dfc1d601                       call 0x142a78270
0000000140d0c091  898744200100                     mov dword ptr [rdi + 0x12044], eax
0000000140d0c097  488bce                           mov rcx, rsi
0000000140d0c09a  e821c0d601                       call 0x142a780c0
0000000140d0c09f  8887f81e0100                     mov byte ptr [rdi + 0x11ef8], al
0000000140d0c0a5  488bce                           mov rcx, rsi
0000000140d0c0a8  e813c0d601                       call 0x142a780c0
0000000140d0c0ad  8887f91e0100                     mov byte ptr [rdi + 0x11ef9], al
0000000140d0c0b3  488bce                           mov rcx, rsi
0000000140d0c0b6  e8b5c1d601                       call 0x142a78270
0000000140d0c0bb  898720cf0100                     mov dword ptr [rdi + 0x1cf20], eax
0000000140d0c0c1  488d97e8be0100                   lea rdx, [rdi + 0x1bee8]
0000000140d0c0c8  4c8bc5                           mov r8, rbp
0000000140d0c0cb  488bce                           mov rcx, rsi
0000000140d0c0ce  e8fdf5feff                       call 0x140cfb6d0
0000000140d0c0d3  488bce                           mov rcx, rsi
0000000140d0c0d6  e8e5bfd601                       call 0x142a780c0
0000000140d0c0db  88870b1c0100                     mov byte ptr [rdi + 0x11c0b], al
0000000140d0c0e1  488bce                           mov rcx, rsi
0000000140d0c0e4  e8d7bfd601                       call 0x142a780c0
0000000140d0c0e9  8887de1b0100                     mov byte ptr [rdi + 0x11bde], al
0000000140d0c0ef  488bce                           mov rcx, rsi
0000000140d0c0f2  e879c1d601                       call 0x142a78270
0000000140d0c0f7  8987e81b0100                     mov dword ptr [rdi + 0x11be8], eax
0000000140d0c0fd  e915030000                       jmp 0x140d0c417
0000000140d0c102  488d97881b0200                   lea rdx, [rdi + 0x21b88]
0000000140d0c109  4c8bc5                           mov r8, rbp
0000000140d0c10c  488bce                           mov rcx, rsi
0000000140d0c10f  e8ac42feff                       call 0x140cf03c0
0000000140d0c114  488bce                           mov rcx, rsi
0000000140d0c117  e8a4bfd601                       call 0x142a780c0
0000000140d0c11c  888715fb0100                     mov byte ptr [rdi + 0x1fb15], al
0000000140d0c122  488bce                           mov rcx, rsi
0000000140d0c125  e896bfd601                       call 0x142a780c0
0000000140d0c12a  888716fb0100                     mov byte ptr [rdi + 0x1fb16], al
0000000140d0c130  488bce                           mov rcx, rsi
0000000140d0c133  e838c1d601                       call 0x142a78270
0000000140d0c138  898718fb0100                     mov dword ptr [rdi + 0x1fb18], eax
0000000140d0c13e  488bce                           mov rcx, rsi
0000000140d0c141  e87abfd601                       call 0x142a780c0
0000000140d0c146  888714fb0100                     mov byte ptr [rdi + 0x1fb14], al
0000000140d0c14c  488bce                           mov rcx, rsi
0000000140d0c14f  e81cc1d601                       call 0x142a78270
0000000140d0c154  8987d01b0100                     mov dword ptr [rdi + 0x11bd0], eax
0000000140d0c15a  488bce                           mov rcx, rsi
0000000140d0c15d  e80ec1d601                       call 0x142a78270
0000000140d0c162  8987d81b0100                     mov dword ptr [rdi + 0x11bd8], eax
0000000140d0c168  488bce                           mov rcx, rsi
0000000140d0c16b  e850bfd601                       call 0x142a780c0
0000000140d0c170  8887dc1b0100                     mov byte ptr [rdi + 0x11bdc], al
0000000140d0c176  488bce                           mov rcx, rsi
0000000140d0c179  e8f2c0d601                       call 0x142a78270
0000000140d0c17e  8987e01b0100                     mov dword ptr [rdi + 0x11be0], eax
0000000140d0c184  488bce                           mov rcx, rsi
0000000140d0c187  e834bfd601                       call 0x142a780c0
0000000140d0c18c  8887dd1b0100                     mov byte ptr [rdi + 0x11bdd], al
0000000140d0c192  488bce                           mov rcx, rsi
0000000140d0c195  e826bfd601                       call 0x142a780c0
0000000140d0c19a  8887fc1b0100                     mov byte ptr [rdi + 0x11bfc], al
0000000140d0c1a0  488bce                           mov rcx, rsi
0000000140d0c1a3  e818bfd601                       call 0x142a780c0
0000000140d0c1a8  8887fe1b0100                     mov byte ptr [rdi + 0x11bfe], al
0000000140d0c1ae  488bce                           mov rcx, rsi
0000000140d0c1b1  e8bac0d601                       call 0x142a78270
0000000140d0c1b6  8987e41b0100                     mov dword ptr [rdi + 0x11be4], eax
0000000140d0c1bc  488bce                           mov rcx, rsi
0000000140d0c1bf  e8fcbed601                       call 0x142a780c0
0000000140d0c1c4  8887f91b0100                     mov byte ptr [rdi + 0x11bf9], al
0000000140d0c1ca  488bce                           mov rcx, rsi
0000000140d0c1cd  e8eebed601                       call 0x142a780c0
0000000140d0c1d2  8887fa1b0100                     mov byte ptr [rdi + 0x11bfa], al
0000000140d0c1d8  488bce                           mov rcx, rsi
0000000140d0c1db  e8e0bed601                       call 0x142a780c0
0000000140d0c1e0  8887fb1b0100                     mov byte ptr [rdi + 0x11bfb], al
0000000140d0c1e6  488bce                           mov rcx, rsi
0000000140d0c1e9  e8d2bed601                       call 0x142a780c0
0000000140d0c1ee  8887031c0100                     mov byte ptr [rdi + 0x11c03], al
0000000140d0c1f4  488bce                           mov rcx, rsi
0000000140d0c1f7  e8c4bed601                       call 0x142a780c0
0000000140d0c1fc  8887041c0100                     mov byte ptr [rdi + 0x11c04], al
0000000140d0c202  488bce                           mov rcx, rsi
0000000140d0c205  e8b6bed601                       call 0x142a780c0
0000000140d0c20a  8887051c0100                     mov byte ptr [rdi + 0x11c05], al
0000000140d0c210  488bce                           mov rcx, rsi
0000000140d0c213  e8a8bed601                       call 0x142a780c0
0000000140d0c218  8887061c0100                     mov byte ptr [rdi + 0x11c06], al
0000000140d0c21e  488bce                           mov rcx, rsi
0000000140d0c221  e89abed601                       call 0x142a780c0
0000000140d0c226  8887071c0100                     mov byte ptr [rdi + 0x11c07], al
0000000140d0c22c  488bce                           mov rcx, rsi
0000000140d0c22f  e88cbed601                       call 0x142a780c0
0000000140d0c234  8887fd1b0100                     mov byte ptr [rdi + 0x11bfd], al
0000000140d0c23a  488bce                           mov rcx, rsi
0000000140d0c23d  e87ebed601                       call 0x142a780c0
0000000140d0c242  8887011c0100                     mov byte ptr [rdi + 0x11c01], al
0000000140d0c248  488bce                           mov rcx, rsi
0000000140d0c24b  e870bed601                       call 0x142a780c0
0000000140d0c250  8887091c0100                     mov byte ptr [rdi + 0x11c09], al
0000000140d0c256  488bce                           mov rcx, rsi
0000000140d0c259  e862bed601                       call 0x142a780c0
0000000140d0c25e  88870a1c0100                     mov byte ptr [rdi + 0x11c0a], al
0000000140d0c264  488bce                           mov rcx, rsi
0000000140d0c267  e854bed601                       call 0x142a780c0
0000000140d0c26c  88870c1c0100                     mov byte ptr [rdi + 0x11c0c], al
0000000140d0c272  488bce                           mov rcx, rsi
0000000140d0c275  e8f6bfd601                       call 0x142a78270
0000000140d0c27a  8987c8130100                     mov dword ptr [rdi + 0x113c8], eax
0000000140d0c280  488bce                           mov rcx, rsi
0000000140d0c283  e8e8bfd601                       call 0x142a78270
0000000140d0c288  898738200100                     mov dword ptr [rdi + 0x12038], eax
0000000140d0c28e  488bce                           mov rcx, rsi
0000000140d0c291  e8dabfd601                       call 0x142a78270
0000000140d0c296  89873c200100                     mov dword ptr [rdi + 0x1203c], eax
0000000140d0c29c  488bce                           mov rcx, rsi
0000000140d0c29f  e81cbed601                       call 0x142a780c0
0000000140d0c2a4  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0c2aa  488bce                           mov rcx, rsi
0000000140d0c2ad  e80ebed601                       call 0x142a780c0
0000000140d0c2b2  8887101c0100                     mov byte ptr [rdi + 0x11c10], al
0000000140d0c2b8  488bce                           mov rcx, rsi
0000000140d0c2bb  e860bfd601                       call 0x142a78220
0000000140d0c2c0  6689870e1c0100                   mov word ptr [rdi + 0x11c0e], ax
0000000140d0c2c7  488bce                           mov rcx, rsi
0000000140d0c2ca  e8f1bdd601                       call 0x142a780c0
0000000140d0c2cf  888740200100                     mov byte ptr [rdi + 0x12040], al
0000000140d0c2d5  488d9728cf0100                   lea rdx, [rdi + 0x1cf28]
0000000140d0c2dc  4c8bc5                           mov r8, rbp
0000000140d0c2df  488bce                           mov rcx, rsi
0000000140d0c2e2  e8e947feff                       call 0x140cf0ad0
0000000140d0c2e7  488d9740cf0100                   lea rdx, [rdi + 0x1cf40]
0000000140d0c2ee  4c8bc5                           mov r8, rbp
0000000140d0c2f1  488bce                           mov rcx, rsi
0000000140d0c2f4  e8c74afeff                       call 0x140cf0dc0
0000000140d0c2f9  488d9708cf0100                   lea rdx, [rdi + 0x1cf08]
0000000140d0c300  4c8bc5                           mov r8, rbp
0000000140d0c303  488bce                           mov rcx, rsi
0000000140d0c306  e8e551feff                       call 0x140cf14f0
0000000140d0c30b  488bce                           mov rcx, rsi
0000000140d0c30e  e8adbdd601                       call 0x142a780c0
0000000140d0c313  8887ec1b0100                     mov byte ptr [rdi + 0x11bec], al
0000000140d0c319  488bce                           mov rcx, rsi
0000000140d0c31c  e84fbfd601                       call 0x142a78270
0000000140d0c321  8987f01b0100                     mov dword ptr [rdi + 0x11bf0], eax
0000000140d0c327  488bce                           mov rcx, rsi
0000000140d0c32a  e841bfd601                       call 0x142a78270
0000000140d0c32f  8987f41b0100                     mov dword ptr [rdi + 0x11bf4], eax
0000000140d0c335  488bce                           mov rcx, rsi
0000000140d0c338  e883bdd601                       call 0x142a780c0
0000000140d0c33d  8887f81b0100                     mov byte ptr [rdi + 0x11bf8], al
0000000140d0c343  488bce                           mov rcx, rsi
0000000140d0c346  e875bdd601                       call 0x142a780c0
0000000140d0c34b  8887081c0100                     mov byte ptr [rdi + 0x11c08], al
0000000140d0c351  488bce                           mov rcx, rsi
0000000140d0c354  e867bdd601                       call 0x142a780c0
0000000140d0c359  88877f1b0200                     mov byte ptr [rdi + 0x21b7f], al
0000000140d0c35f  488bce                           mov rcx, rsi
0000000140d0c362  e809bfd601                       call 0x142a78270
0000000140d0c367  448bf0                           mov r14d, eax
0000000140d0c36a  488bce                           mov rcx, rsi
0000000140d0c36d  e8febed601                       call 0x142a78270
0000000140d0c372  448bf8                           mov r15d, eax
0000000140d0c375  488bce                           mov rcx, rsi
0000000140d0c378  e8f3bed601                       call 0x142a78270
0000000140d0c37d  898744200100                     mov dword ptr [rdi + 0x12044], eax
0000000140d0c383  488bce                           mov rcx, rsi
0000000140d0c386  e835bdd601                       call 0x142a780c0
0000000140d0c38b  8887f81e0100                     mov byte ptr [rdi + 0x11ef8], al
0000000140d0c391  488bce                           mov rcx, rsi
0000000140d0c394  e827bdd601                       call 0x142a780c0
0000000140d0c399  8887f91e0100                     mov byte ptr [rdi + 0x11ef9], al
0000000140d0c39f  488bce                           mov rcx, rsi
0000000140d0c3a2  e8c9bed601                       call 0x142a78270
0000000140d0c3a7  898720cf0100                     mov dword ptr [rdi + 0x1cf20], eax
0000000140d0c3ad  488d97e8be0100                   lea rdx, [rdi + 0x1bee8]
0000000140d0c3b4  4c8bc5                           mov r8, rbp
0000000140d0c3b7  488bce                           mov rcx, rsi
0000000140d0c3ba  e811f3feff                       call 0x140cfb6d0
0000000140d0c3bf  488bce                           mov rcx, rsi
0000000140d0c3c2  e8f9bcd601                       call 0x142a780c0
0000000140d0c3c7  88870b1c0100                     mov byte ptr [rdi + 0x11c0b], al
0000000140d0c3cd  488bce                           mov rcx, rsi
0000000140d0c3d0  e8ebbcd601                       call 0x142a780c0
0000000140d0c3d5  8887de1b0100                     mov byte ptr [rdi + 0x11bde], al
0000000140d0c3db  488bce                           mov rcx, rsi
0000000140d0c3de  e88dbed601                       call 0x142a78270
0000000140d0c3e3  8987e81b0100                     mov dword ptr [rdi + 0x11be8], eax
0000000140d0c3e9  488d97781e0100                   lea rdx, [rdi + 0x11e78]
0000000140d0c3f0  4c8bc5                           mov r8, rbp
0000000140d0c3f3  488bce                           mov rcx, rsi
0000000140d0c3f6  e8f5f6feff                       call 0x140cfbaf0
0000000140d0c3fb  488bce                           mov rcx, rsi
0000000140d0c3fe  e86dbed601                       call 0x142a78270
0000000140d0c403  8987141c0100                     mov dword ptr [rdi + 0x11c14], eax
0000000140d0c409  488bce                           mov rcx, rsi
0000000140d0c40c  e85fbed601                       call 0x142a78270
0000000140d0c411  8987181c0100                     mov dword ptr [rdi + 0x11c18], eax
0000000140d0c417  458bc6                           mov r8d, r14d
0000000140d0c41a  33d2                             xor edx, edx
0000000140d0c41c  488b8f18130100                   mov rcx, qword ptr [rdi + 0x11318]
0000000140d0c423  e8d8c6fdff                       call 0x140ce8b00
0000000140d0c428  458bc7                           mov r8d, r15d
0000000140d0c42b  ba01000000                       mov edx, 1
0000000140d0c430  488b8f18130100                   mov rcx, qword ptr [rdi + 0x11318]
0000000140d0c437  e8c4c6fdff                       call 0x140ce8b00
0000000140d0c43c  b8a1000000                       mov eax, 0xa1
0000000140d0c441  66443be0                         cmp r12w, ax
0000000140d0c445  0f878f000000                     ja 0x140d0c4da
0000000140d0c44b  c744242000000000                 mov dword ptr [rsp + 0x20], 0
0000000140d0c453  488d9f381c0100                   lea rbx, [rdi + 0x11c38]
0000000140d0c45a  4c8bcd                           mov r9, rbp
0000000140d0c45d  4c8d442420                       lea r8, [rsp + 0x20]
0000000140d0c462  488bd3                           mov rdx, rbx
0000000140d0c465  488bce                           mov rcx, rsi
0000000140d0c468  e813f3feff                       call 0x140cfb780
0000000140d0c46d  488d4ff0                         lea rcx, [rdi - 0x10]
0000000140d0c471  e8fa89c8ff                       call 0x140994e70
0000000140d0c476  84c0                             test al, al
0000000140d0c478  7460                             je 0x140d0c4da
0000000140d0c47a  488bcb                           mov rcx, rbx
0000000140d0c47d  e81e57a2ff                       call 0x140731ba0
0000000140d0c482  84c0                             test al, al
0000000140d0c484  7554                             jne 0x140d0c4da
0000000140d0c486  837c242001                       cmp dword ptr [rsp + 0x20], 1
0000000140d0c48b  754d                             jne 0x140d0c4da
0000000140d0c48d  488bd3                           mov rdx, rbx
0000000140d0c490  488d4c2448                       lea rcx, [rsp + 0x48]
0000000140d0c495  e8a6639fff                       call 0x140702840
0000000140d0c49a  90                               nop
0000000140d0c49b  488d4ff0                         lea rcx, [rdi - 0x10]
0000000140d0c49f  e8ec49c9ff                       call 0x1409a0e90
0000000140d0c4a4  488bd0                           mov rdx, rax
0000000140d0c4a7  4533c0                           xor r8d, r8d
0000000140d0c4aa  488d4c2448                       lea rcx, [rsp + 0x48]
0000000140d0c4af  e83c0cc8ff                       call 0x14098d0f0
0000000140d0c4b4  84c0                             test al, al
0000000140d0c4b6  7418                             je 0x140d0c4d0
0000000140d0c4b8  488d442448                       lea rax, [rsp + 0x48]
0000000140d0c4bd  483bc3                           cmp rax, rbx
0000000140d0c4c0  740e                             je 0x140d0c4d0
0000000140d0c4c2  488d542448                       lea rdx, [rsp + 0x48]
0000000140d0c4c7  488bcb                           mov rcx, rbx
0000000140d0c4ca  e88180a0ff                       call 0x140714550
0000000140d0c4cf  90                               nop
0000000140d0c4d0  488d4c2448                       lea rcx, [rsp + 0x48]
0000000140d0c4d5  e866cd9fff                       call 0x140709240
0000000140d0c4da  488b8c2468010000                 mov rcx, qword ptr [rsp + 0x168]
0000000140d0c4e2  4833cc                           xor rcx, rsp
0000000140d0c4e5  e8a6d06b03                       call 0x1443c9590
0000000140d0c4ea  4881c470010000                   add rsp, 0x170
0000000140d0c4f1  415f                             pop r15
0000000140d0c4f3  415e                             pop r14
0000000140d0c4f5  415c                             pop r12
0000000140d0c4f7  5f                               pop rdi
0000000140d0c4f8  5e                               pop rsi
0000000140d0c4f9  5d                               pop rbp
0000000140d0c4fa  5b                               pop rbx
0000000140d0c4fb  c3                               ret
0000000140d0c4fc  488d4c2428                       lea rcx, [rsp + 0x28]
0000000140d0c501  e84ae6aeff                       call 0x1407fab50
0000000140d0c506  488d15cbab1209                   lea rdx, [rip + 0x912abcb]
0000000140d0c50d  488d4c2428                       lea rcx, [rsp + 0x28]
0000000140d0c512  e8bf4a6c03                       call 0x1443d0fd6
0000000140d0c517  cc                               int3
0000000140d0c518  42a3d000bca5d0008aa8             movabs dword ptr [0xa88a00d0a5bc00d0], eax
0000000140d0c522  d000                             rol byte ptr [rax], 1
0000000140d0c524  98                               cwde
0000000140d0c525  aa                               stosb byte ptr [rdi], al
0000000140d0c526  d000                             rol byte ptr [rax], 1
0000000140d0c528  a6                               cmpsb byte ptr [rsi], byte ptr [rdi]
0000000140d0c529  ac                               lodsb al, byte ptr [rsi]
0000000140d0c52a  d000                             rol byte ptr [rax], 1

; prefix_array_A_reader
0000000140cf0ad0  48895c2420                       mov qword ptr [rsp + 0x20], rbx
0000000140cf0ad5  55                               push rbp
0000000140cf0ad6  56                               push rsi
0000000140cf0ad7  57                               push rdi
0000000140cf0ad8  4154                             push r12
0000000140cf0ada  4155                             push r13
0000000140cf0adc  4156                             push r14
0000000140cf0ade  4157                             push r15
0000000140cf0ae0  488d6c24d9                       lea rbp, [rsp - 0x27]
0000000140cf0ae5  4881ecb0000000                   sub rsp, 0xb0
0000000140cf0aec  488b05cd4d5b09                   mov rax, qword ptr [rip + 0x95b4dcd]
0000000140cf0af3  4833c4                           xor rax, rsp
0000000140cf0af6  48894517                         mov qword ptr [rbp + 0x17], rax
0000000140cf0afa  4d8bf8                           mov r15, r8
0000000140cf0afd  488bf2                           mov rsi, rdx
0000000140cf0b00  488bf9                           mov rdi, rcx
0000000140cf0b03  e86877d801                       call 0x142a78270
0000000140cf0b08  448be0                           mov r12d, eax
0000000140cf0b0b  4533ed                           xor r13d, r13d
0000000140cf0b0e  458bf5                           mov r14d, r13d
0000000140cf0b11  85c0                             test eax, eax
0000000140cf0b13  0f8419020000                     je 0x140cf0d32
0000000140cf0b19  488d15808e80ff                   lea rdx, [rip - 0x7f7180]
0000000140cf0b20  488d05a1521804                   lea rax, [rip + 0x41852a1]
0000000140cf0b27  488945b7                         mov qword ptr [rbp - 0x49], rax
0000000140cf0b2b  0f57c0                           xorps xmm0, xmm0
0000000140cf0b2e  0f1145bf                         movups xmmword ptr [rbp - 0x41], xmm0
0000000140cf0b32  4c896dcf                         mov qword ptr [rbp - 0x31], r13
0000000140cf0b36  48c745d70f000000                 mov qword ptr [rbp - 0x29], 0xf
0000000140cf0b3e  c645bf00                         mov byte ptr [rbp - 0x41], 0
0000000140cf0b42  660f7f45e7                       movdqa xmmword ptr [rbp - 0x19], xmm0
0000000140cf0b47  4c896df7                         mov qword ptr [rbp - 9], r13
0000000140cf0b4b  66c7450f0000                     mov word ptr [rbp + 0xf], 0
0000000140cf0b51  c6450301                         mov byte ptr [rbp + 3], 1
0000000140cf0b55  6644896d05                       mov word ptr [rbp + 5], r13w
0000000140cf0b5a  c745ff0000003f                   mov dword ptr [rbp - 1], 0x3f000000
0000000140cf0b61  c7450700000000                   mov dword ptr [rbp + 7], 0
0000000140cf0b68  c7450bc3f5a83e                   mov dword ptr [rbp + 0xb], 0x3ea8f5c3
0000000140cf0b6f  488b4030                         mov rax, qword ptr [rax + 0x30]
0000000140cf0b73  483bc2                           cmp rax, rdx
0000000140cf0b76  7406                             je 0x140cf0b7e
0000000140cf0b78  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0b7c  ffd0                             call rax
0000000140cf0b7e  488bcf                           mov rcx, rdi
0000000140cf0b81  e83a75d801                       call 0x142a780c0
0000000140cf0b86  0fb6d8                           movzx ebx, al
0000000140cf0b89  488b4db7                         mov rcx, qword ptr [rbp - 0x49]
0000000140cf0b8d  488b5118                         mov rdx, qword ptr [rcx + 0x18]
0000000140cf0b91  488d0598e781ff                   lea rax, [rip - 0x7e1868]
0000000140cf0b98  483bd0                           cmp rdx, rax
0000000140cf0b9b  7504                             jne 0x140cf0ba1
0000000140cf0b9d  33c0                             xor eax, eax
0000000140cf0b9f  eb06                             jmp 0x140cf0ba7
0000000140cf0ba1  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0ba5  ffd2                             call rdx
0000000140cf0ba7  3ad8                             cmp bl, al
0000000140cf0ba9  0f85ce010000                     jne 0x140cf0d7d
0000000140cf0baf  488bcf                           mov rcx, rdi
0000000140cf0bb2  e86976d801                       call 0x142a78220
0000000140cf0bb7  0fb7d8                           movzx ebx, ax
0000000140cf0bba  488b4db7                         mov rcx, qword ptr [rbp - 0x49]
0000000140cf0bbe  488b4110                         mov rax, qword ptr [rcx + 0x10]
0000000140cf0bc2  488d15d72b89ff                   lea rdx, [rip - 0x76d429]
0000000140cf0bc9  483bc2                           cmp rax, rdx
0000000140cf0bcc  7507                             jne 0x140cf0bd5
0000000140cf0bce  b850000000                       mov eax, 0x50
0000000140cf0bd3  eb0a                             jmp 0x140cf0bdf
0000000140cf0bd5  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0bd9  ffd0                             call rax
0000000140cf0bdb  488b4db7                         mov rcx, qword ptr [rbp - 0x49]
0000000140cf0bdf  663bd8                           cmp bx, ax
0000000140cf0be2  0f877b010000                     ja 0x140cf0d63
0000000140cf0be8  488b4140                         mov rax, qword ptr [rcx + 0x40]
0000000140cf0bec  488d0dfde10000                   lea rcx, [rip + 0xe1fd]
0000000140cf0bf3  4d8bcf                           mov r9, r15
0000000140cf0bf6  440fb7c3                         movzx r8d, bx
0000000140cf0bfa  488bd7                           mov rdx, rdi
0000000140cf0bfd  483bc1                           cmp rax, rcx
0000000140cf0c00  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0c04  7507                             jne 0x140cf0c0d
0000000140cf0c06  e8e5e10000                       call 0x140cfedf0
0000000140cf0c0b  eb02                             jmp 0x140cf0c0f
0000000140cf0c0d  ffd0                             call rax
0000000140cf0c0f  488b4708                         mov rax, qword ptr [rdi + 8]
0000000140cf0c13  4885c0                           test rax, rax
0000000140cf0c16  0f847b010000                     je 0x140cf0d97
0000000140cf0c1c  8b480c                           mov ecx, dword ptr [rax + 0xc]
0000000140cf0c1f  83f903                           cmp ecx, 3
0000000140cf0c22  7408                             je 0x140cf0c2c
0000000140cf0c24  85c9                             test ecx, ecx
0000000140cf0c26  7404                             je 0x140cf0c2c
0000000140cf0c28  32c0                             xor al, al
0000000140cf0c2a  eb02                             jmp 0x140cf0c2e
0000000140cf0c2c  b001                             mov al, 1
0000000140cf0c2e  84c0                             test al, al
0000000140cf0c30  0f8561010000                     jne 0x140cf0d97
0000000140cf0c36  488b45b7                         mov rax, qword ptr [rbp - 0x49]
0000000140cf0c3a  4c8b4838                         mov r9, qword ptr [rax + 0x38]
0000000140cf0c3e  488d055b8d80ff                   lea rax, [rip - 0x7f72a5]
0000000140cf0c45  4c3bc8                           cmp r9, rax
0000000140cf0c48  740d                             je 0x140cf0c57
0000000140cf0c4a  4d8bc7                           mov r8, r15
0000000140cf0c4d  0fb7d3                           movzx edx, bx
0000000140cf0c50  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0c54  41ffd1                           call r9
0000000140cf0c57  488b4608                         mov rax, qword ptr [rsi + 8]
0000000140cf0c5b  483b4610                         cmp rax, qword ptr [rsi + 0x10]
0000000140cf0c5f  7413                             je 0x140cf0c74
0000000140cf0c61  488d55b7                         lea rdx, [rbp - 0x49]
0000000140cf0c65  488bc8                           mov rcx, rax
0000000140cf0c68  e8432b0000                       call 0x140cf37b0
0000000140cf0c6d  4883460860                       add qword ptr [rsi + 8], 0x60
0000000140cf0c72  eb10                             jmp 0x140cf0c84
0000000140cf0c74  4c8d45b7                         lea r8, [rbp - 0x49]
0000000140cf0c78  488bd0                           mov rdx, rax
0000000140cf0c7b  488bce                           mov rcx, rsi
0000000140cf0c7e  e8fd1e0000                       call 0x140cf2b80
0000000140cf0c83  90                               nop
0000000140cf0c84  4c8b45e7                         mov r8, qword ptr [rbp - 0x19]
0000000140cf0c88  4d85c0                           test r8, r8
0000000140cf0c8b  7462                             je 0x140cf0cef
0000000140cf0c8d  488b4df7                         mov rcx, qword ptr [rbp - 9]
0000000140cf0c91  492bc8                           sub rcx, r8
0000000140cf0c94  48b8abaaaaaaaaaaaa2a             movabs rax, 0x2aaaaaaaaaaaaaab
0000000140cf0c9e  48f7e9                           imul rcx
0000000140cf0ca1  48d1fa                           sar rdx, 1
0000000140cf0ca4  488bc2                           mov rax, rdx
0000000140cf0ca7  48c1e83f                         shr rax, 0x3f
0000000140cf0cab  4803d0                           add rdx, rax
0000000140cf0cae  488d1452                         lea rdx, [rdx + rdx*2]
0000000140cf0cb2  48c1e202                         shl rdx, 2
0000000140cf0cb6  498bc0                           mov rax, r8
0000000140cf0cb9  4881fa00100000                   cmp rdx, 0x1000
0000000140cf0cc0  7219                             jb 0x140cf0cdb
0000000140cf0cc2  4883c227                         add rdx, 0x27
0000000140cf0cc6  4d8b40f8                         mov r8, qword ptr [r8 - 8]
0000000140cf0cca  492bc0                           sub rax, r8
0000000140cf0ccd  4883c0f8                         add rax, -8
0000000140cf0cd1  4883f81f                         cmp rax, 0x1f
0000000140cf0cd5  0f8781000000                     ja 0x140cf0d5c
0000000140cf0cdb  498bc8                           mov rcx, r8
0000000140cf0cde  e8ed846d03                       call 0x1443c91d0
0000000140cf0ce3  0f57c0                           xorps xmm0, xmm0
0000000140cf0ce6  660f7f45e7                       movdqa xmmword ptr [rbp - 0x19], xmm0
0000000140cf0ceb  4c896df7                         mov qword ptr [rbp - 9], r13
0000000140cf0cef  488b55d7                         mov rdx, qword ptr [rbp - 0x29]
0000000140cf0cf3  4883fa0f                         cmp rdx, 0xf
0000000140cf0cf7  762d                             jbe 0x140cf0d26
0000000140cf0cf9  48ffc2                           inc rdx
0000000140cf0cfc  488b4dbf                         mov rcx, qword ptr [rbp - 0x41]
0000000140cf0d00  488bc1                           mov rax, rcx
0000000140cf0d03  4881fa00100000                   cmp rdx, 0x1000
0000000140cf0d0a  7215                             jb 0x140cf0d21
0000000140cf0d0c  4883c227                         add rdx, 0x27
0000000140cf0d10  488b49f8                         mov rcx, qword ptr [rcx - 8]
0000000140cf0d14  482bc1                           sub rax, rcx
0000000140cf0d17  4883c0f8                         add rax, -8
0000000140cf0d1b  4883f81f                         cmp rax, 0x1f
0000000140cf0d1f  773b                             ja 0x140cf0d5c
0000000140cf0d21  e8aa846d03                       call 0x1443c91d0
0000000140cf0d26  41ffc6                           inc r14d
0000000140cf0d29  453bf4                           cmp r14d, r12d
0000000140cf0d2c  0f82e7fdffff                     jb 0x140cf0b19
0000000140cf0d32  418bc6                           mov eax, r14d
0000000140cf0d35  488b4d17                         mov rcx, qword ptr [rbp + 0x17]
0000000140cf0d39  4833cc                           xor rcx, rsp
0000000140cf0d3c  e84f886d03                       call 0x1443c9590
0000000140cf0d41  488b9c2408010000                 mov rbx, qword ptr [rsp + 0x108]
0000000140cf0d49  4881c4b0000000                   add rsp, 0xb0
0000000140cf0d50  415f                             pop r15
0000000140cf0d52  415e                             pop r14
0000000140cf0d54  415d                             pop r13
0000000140cf0d56  415c                             pop r12
0000000140cf0d58  5f                               pop rdi
0000000140cf0d59  5e                               pop rsi
0000000140cf0d5a  5d                               pop rbp
0000000140cf0d5b  c3                               ret
0000000140cf0d5c  ff153e03a003                     call qword ptr [rip + 0x3a0033e]
0000000140cf0d62  90                               nop
0000000140cf0d63  488d4d97                         lea rcx, [rbp - 0x69]
0000000140cf0d67  e8f461b0ff                       call 0x1407f6f60
0000000140cf0d6c  488d1515621409                   lea rdx, [rip + 0x9146215]
0000000140cf0d73  488d4d97                         lea rcx, [rbp - 0x69]
0000000140cf0d77  e85a026e03                       call 0x1443d0fd6
0000000140cf0d7c  cc                               int3
0000000140cf0d7d  488d4d97                         lea rcx, [rbp - 0x69]
0000000140cf0d81  e8ba9eb0ff                       call 0x1407fac40
0000000140cf0d86  488d1563611409                   lea rdx, [rip + 0x9146163]
0000000140cf0d8d  488d4d97                         lea rcx, [rbp - 0x69]
0000000140cf0d91  e840026e03                       call 0x1443d0fd6
0000000140cf0d96  cc                               int3
0000000140cf0d97  488d4d97                         lea rcx, [rbp - 0x69]
0000000140cf0d9b  e8309cb0ff                       call 0x1407fa9d0
0000000140cf0da0  488d1551621409                   lea rdx, [rip + 0x9146251]
0000000140cf0da7  488d4d97                         lea rcx, [rbp - 0x69]
0000000140cf0dab  e826026e03                       call 0x1443d0fd6
0000000140cf0db0  cc                               int3
0000000140cf0db1  cc                               int3
0000000140cf0db2  cc                               int3
0000000140cf0db3  cc                               int3
0000000140cf0db4  cc                               int3
0000000140cf0db5  cc                               int3
0000000140cf0db6  cc                               int3
0000000140cf0db7  cc                               int3
0000000140cf0db8  cc                               int3
0000000140cf0db9  cc                               int3
0000000140cf0dba  cc                               int3
0000000140cf0dbb  cc                               int3
0000000140cf0dbc  cc                               int3
0000000140cf0dbd  cc                               int3
0000000140cf0dbe  cc                               int3
0000000140cf0dbf  cc                               int3

; prefix_array_B_reader
0000000140cf0dc0  48895c2420                       mov qword ptr [rsp + 0x20], rbx
0000000140cf0dc5  55                               push rbp
0000000140cf0dc6  56                               push rsi
0000000140cf0dc7  57                               push rdi
0000000140cf0dc8  4154                             push r12
0000000140cf0dca  4155                             push r13
0000000140cf0dcc  4156                             push r14
0000000140cf0dce  4157                             push r15
0000000140cf0dd0  488dac2460ffffff                 lea rbp, [rsp - 0xa0]
0000000140cf0dd8  4881eca0010000                   sub rsp, 0x1a0
0000000140cf0ddf  0f29b42490010000                 movaps xmmword ptr [rsp + 0x190], xmm6
0000000140cf0de7  488b05d24a5b09                   mov rax, qword ptr [rip + 0x95b4ad2]
0000000140cf0dee  4833c4                           xor rax, rsp
0000000140cf0df1  48898580000000                   mov qword ptr [rbp + 0x80], rax
0000000140cf0df8  4d8bf8                           mov r15, r8
0000000140cf0dfb  488bf2                           mov rsi, rdx
0000000140cf0dfe  488bf9                           mov rdi, rcx
0000000140cf0e01  e86a74d801                       call 0x142a78270
0000000140cf0e06  448be0                           mov r12d, eax
0000000140cf0e09  33c0                             xor eax, eax
0000000140cf0e0b  448bf0                           mov r14d, eax
0000000140cf0e0e  4585e4                           test r12d, r12d
0000000140cf0e11  0f8439020000                     je 0x140cf1050
0000000140cf0e17  4c8d2daa40a203                   lea r13, [rip + 0x3a240aa]
0000000140cf0e1e  660f6f357abda203                 movdqa xmm6, xmmword ptr [rip + 0x3a2bd7a]
0000000140cf0e26  488d15738b80ff                   lea rdx, [rip - 0x7f748d]
0000000140cf0e2d  4c896c2440                       mov qword ptr [rsp + 0x40], r13
0000000140cf0e32  0f57c0                           xorps xmm0, xmm0
0000000140cf0e35  0f11442450                       movups xmmword ptr [rsp + 0x50], xmm0
0000000140cf0e3a  4889442460                       mov qword ptr [rsp + 0x60], rax
0000000140cf0e3f  48c74424680f000000               mov qword ptr [rsp + 0x68], 0xf
0000000140cf0e48  c644245000                       mov byte ptr [rsp + 0x50], 0
0000000140cf0e4d  4889442470                       mov qword ptr [rsp + 0x70], rax
0000000140cf0e52  48c7442478ffffffff               mov qword ptr [rsp + 0x78], 0xffffffffffffffff
0000000140cf0e5b  660f7f7580                       movdqa xmmword ptr [rbp - 0x80], xmm6
0000000140cf0e60  660f7f7590                       movdqa xmmword ptr [rbp - 0x70], xmm6
0000000140cf0e65  660f7f75a0                       movdqa xmmword ptr [rbp - 0x60], xmm6
0000000140cf0e6a  660f7f75b0                       movdqa xmmword ptr [rbp - 0x50], xmm6
0000000140cf0e6f  660f7f75c0                       movdqa xmmword ptr [rbp - 0x40], xmm6
0000000140cf0e74  660f7f75d0                       movdqa xmmword ptr [rbp - 0x30], xmm6
0000000140cf0e79  660f7f75e0                       movdqa xmmword ptr [rbp - 0x20], xmm6
0000000140cf0e7e  660f7f75f0                       movdqa xmmword ptr [rbp - 0x10], xmm6
0000000140cf0e83  660f7f7500                       movdqa xmmword ptr [rbp], xmm6
0000000140cf0e88  660f7f7510                       movdqa xmmword ptr [rbp + 0x10], xmm6
0000000140cf0e8d  660f7f7520                       movdqa xmmword ptr [rbp + 0x20], xmm6
0000000140cf0e92  660f7f7530                       movdqa xmmword ptr [rbp + 0x30], xmm6
0000000140cf0e97  660f7f7540                       movdqa xmmword ptr [rbp + 0x40], xmm6
0000000140cf0e9c  660f7f7550                       movdqa xmmword ptr [rbp + 0x50], xmm6
0000000140cf0ea1  660f7f7560                       movdqa xmmword ptr [rbp + 0x60], xmm6
0000000140cf0ea6  48c74570ffffffff                 mov qword ptr [rbp + 0x70], 0xffffffffffffffff
0000000140cf0eae  89442448                         mov dword ptr [rsp + 0x48], eax
0000000140cf0eb2  c744244cffffffff                 mov dword ptr [rsp + 0x4c], 0xffffffff
0000000140cf0eba  498b4530                         mov rax, qword ptr [r13 + 0x30]
0000000140cf0ebe  483bc2                           cmp rax, rdx
0000000140cf0ec1  7407                             je 0x140cf0eca
0000000140cf0ec3  488d4c2440                       lea rcx, [rsp + 0x40]
0000000140cf0ec8  ffd0                             call rax
0000000140cf0eca  488bcf                           mov rcx, rdi
0000000140cf0ecd  e8ee71d801                       call 0x142a780c0
0000000140cf0ed2  0fb6d8                           movzx ebx, al
0000000140cf0ed5  488b4c2440                       mov rcx, qword ptr [rsp + 0x40]
0000000140cf0eda  488b5118                         mov rdx, qword ptr [rcx + 0x18]
0000000140cf0ede  488d054be481ff                   lea rax, [rip - 0x7e1bb5]
0000000140cf0ee5  483bd0                           cmp rdx, rax
0000000140cf0ee8  7504                             jne 0x140cf0eee
0000000140cf0eea  33c0                             xor eax, eax
0000000140cf0eec  eb07                             jmp 0x140cf0ef5
0000000140cf0eee  488d4c2440                       lea rcx, [rsp + 0x40]
0000000140cf0ef3  ffd2                             call rdx
0000000140cf0ef5  3ad8                             cmp bl, al
0000000140cf0ef7  0f8588010000                     jne 0x140cf1085
0000000140cf0efd  488bcf                           mov rcx, rdi
0000000140cf0f00  e81b73d801                       call 0x142a78220
0000000140cf0f05  0fb7d8                           movzx ebx, ax
0000000140cf0f08  488b4c2440                       mov rcx, qword ptr [rsp + 0x40]
0000000140cf0f0d  488b4110                         mov rax, qword ptr [rcx + 0x10]
0000000140cf0f11  488d15882889ff                   lea rdx, [rip - 0x76d778]
0000000140cf0f18  483bc2                           cmp rax, rdx
0000000140cf0f1b  7507                             jne 0x140cf0f24
0000000140cf0f1d  b850000000                       mov eax, 0x50
0000000140cf0f22  eb0c                             jmp 0x140cf0f30
0000000140cf0f24  488d4c2440                       lea rcx, [rsp + 0x40]
0000000140cf0f29  ffd0                             call rax
0000000140cf0f2b  488b4c2440                       mov rcx, qword ptr [rsp + 0x40]
0000000140cf0f30  663bd8                           cmp bx, ax
0000000140cf0f33  0f8784010000                     ja 0x140cf10bd
0000000140cf0f39  488b4140                         mov rax, qword ptr [rcx + 0x40]
0000000140cf0f3d  488d0d5cdf0000                   lea rcx, [rip + 0xdf5c]
0000000140cf0f44  4d8bcf                           mov r9, r15
0000000140cf0f47  440fb7c3                         movzx r8d, bx
0000000140cf0f4b  488bd7                           mov rdx, rdi
0000000140cf0f4e  483bc1                           cmp rax, rcx
0000000140cf0f51  488d4c2440                       lea rcx, [rsp + 0x40]
0000000140cf0f56  7507                             jne 0x140cf0f5f
0000000140cf0f58  e843df0000                       call 0x140cfeea0
0000000140cf0f5d  eb02                             jmp 0x140cf0f61
0000000140cf0f5f  ffd0                             call rax
0000000140cf0f61  488b4708                         mov rax, qword ptr [rdi + 8]
0000000140cf0f65  4885c0                           test rax, rax
0000000140cf0f68  0f8433010000                     je 0x140cf10a1
0000000140cf0f6e  8b480c                           mov ecx, dword ptr [rax + 0xc]
0000000140cf0f71  83f903                           cmp ecx, 3
0000000140cf0f74  7408                             je 0x140cf0f7e
0000000140cf0f76  85c9                             test ecx, ecx
0000000140cf0f78  7404                             je 0x140cf0f7e
0000000140cf0f7a  32c0                             xor al, al
0000000140cf0f7c  eb02                             jmp 0x140cf0f80
0000000140cf0f7e  b001                             mov al, 1
0000000140cf0f80  84c0                             test al, al
0000000140cf0f82  0f8519010000                     jne 0x140cf10a1
0000000140cf0f88  488b442440                       mov rax, qword ptr [rsp + 0x40]
0000000140cf0f8d  4c8b4838                         mov r9, qword ptr [rax + 0x38]
0000000140cf0f91  488d05088a80ff                   lea rax, [rip - 0x7f75f8]
0000000140cf0f98  4c3bc8                           cmp r9, rax
0000000140cf0f9b  740e                             je 0x140cf0fab
0000000140cf0f9d  4d8bc7                           mov r8, r15
0000000140cf0fa0  0fb7d3                           movzx edx, bx
0000000140cf0fa3  488d4c2440                       lea rcx, [rsp + 0x40]
0000000140cf0fa8  41ffd1                           call r9
0000000140cf0fab  488b4608                         mov rax, qword ptr [rsi + 8]
0000000140cf0faf  483b4610                         cmp rax, qword ptr [rsi + 0x10]
0000000140cf0fb3  7417                             je 0x140cf0fcc
0000000140cf0fb5  488d542440                       lea rdx, [rsp + 0x40]
0000000140cf0fba  488bc8                           mov rcx, rax
0000000140cf0fbd  e82e14a1ff                       call 0x1407023f0
0000000140cf0fc2  4881460840010000                 add qword ptr [rsi + 8], 0x140
0000000140cf0fca  eb11                             jmp 0x140cf0fdd
0000000140cf0fcc  4c8d442440                       lea r8, [rsp + 0x40]
0000000140cf0fd1  488bd0                           mov rdx, rax
0000000140cf0fd4  488bce                           mov rcx, rsi
0000000140cf0fd7  e884f7adff                       call 0x1407d0760
0000000140cf0fdc  90                               nop
0000000140cf0fdd  4c896c2440                       mov qword ptr [rsp + 0x40], r13
0000000140cf0fe2  488b5c2470                       mov rbx, qword ptr [rsp + 0x70]
0000000140cf0fe7  4885db                           test rbx, rbx
0000000140cf0fea  7415                             je 0x140cf1001
0000000140cf0fec  488bcb                           mov rcx, rbx
0000000140cf0fef  e8fc5fd8ff                       call 0x140a76ff0
0000000140cf0ff4  ba08080200                       mov edx, 0x20808
0000000140cf0ff9  488bcb                           mov rcx, rbx
0000000140cf0ffc  e8cf816d03                       call 0x1443c91d0
0000000140cf1001  488b542468                       mov rdx, qword ptr [rsp + 0x68]
0000000140cf1006  4883fa0f                         cmp rdx, 0xf
0000000140cf100a  762e                             jbe 0x140cf103a
0000000140cf100c  48ffc2                           inc rdx
0000000140cf100f  488b4c2450                       mov rcx, qword ptr [rsp + 0x50]
0000000140cf1014  488bc1                           mov rax, rcx
0000000140cf1017  4881fa00100000                   cmp rdx, 0x1000
0000000140cf101e  7215                             jb 0x140cf1035
0000000140cf1020  4883c227                         add rdx, 0x27
0000000140cf1024  488b49f8                         mov rcx, qword ptr [rcx - 8]
0000000140cf1028  482bc1                           sub rax, rcx
0000000140cf102b  4883c0f8                         add rax, -8
0000000140cf102f  4883f81f                         cmp rax, 0x1f
0000000140cf1033  7714                             ja 0x140cf1049
0000000140cf1035  e896816d03                       call 0x1443c91d0
0000000140cf103a  41ffc6                           inc r14d
0000000140cf103d  453bf4                           cmp r14d, r12d
0000000140cf1040  730e                             jae 0x140cf1050
0000000140cf1042  33c0                             xor eax, eax
0000000140cf1044  e9ddfdffff                       jmp 0x140cf0e26
0000000140cf1049  ff155100a003                     call qword ptr [rip + 0x3a00051]
0000000140cf104f  cc                               int3
0000000140cf1050  418bc6                           mov eax, r14d
0000000140cf1053  488b8d80000000                   mov rcx, qword ptr [rbp + 0x80]
0000000140cf105a  4833cc                           xor rcx, rsp
0000000140cf105d  e82e856d03                       call 0x1443c9590
0000000140cf1062  488b9c24f8010000                 mov rbx, qword ptr [rsp + 0x1f8]
0000000140cf106a  0f28b42490010000                 movaps xmm6, xmmword ptr [rsp + 0x190]
0000000140cf1072  4881c4a0010000                   add rsp, 0x1a0
0000000140cf1079  415f                             pop r15
0000000140cf107b  415e                             pop r14
0000000140cf107d  415d                             pop r13
0000000140cf107f  415c                             pop r12
0000000140cf1081  5f                               pop rdi
0000000140cf1082  5e                               pop rsi
0000000140cf1083  5d                               pop rbp
0000000140cf1084  c3                               ret
0000000140cf1085  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cf108a  e8b19bb0ff                       call 0x1407fac40
0000000140cf108f  488d155a5e1409                   lea rdx, [rip + 0x9145e5a]
0000000140cf1096  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cf109b  e836ff6d03                       call 0x1443d0fd6
0000000140cf10a0  cc                               int3
0000000140cf10a1  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cf10a6  e82599b0ff                       call 0x1407fa9d0

; prefix_A_public
0000000140cfedf0  48895c2408                       mov qword ptr [rsp + 8], rbx
0000000140cfedf5  4889742410                       mov qword ptr [rsp + 0x10], rsi
0000000140cfedfa  57                               push rdi
0000000140cfedfb  4883ec40                         sub rsp, 0x40
0000000140cfedff  498bf1                           mov rsi, r9
0000000140cfee02  488bda                           mov rbx, rdx
0000000140cfee05  488bf9                           mov rdi, rcx
0000000140cfee08  664183f850                       cmp r8w, 0x50
0000000140cfee0d  7568                             jne 0x140cfee77
0000000140cfee0f  488bca                           mov rcx, rdx
0000000140cfee12  e85994d701                       call 0x142a78270
0000000140cfee17  488d5708                         lea rdx, [rdi + 8]
0000000140cfee1b  894728                           mov dword ptr [rdi + 0x28], eax
0000000140cfee1e  4c8bc6                           mov r8, rsi
0000000140cfee21  488bcb                           mov rcx, rbx
0000000140cfee24  e8c7ccffff                       call 0x140cfbaf0
0000000140cfee29  488bcb                           mov rcx, rbx
0000000140cfee2c  e8bf92d701                       call 0x142a780f0
0000000140cfee31  488bcb                           mov rcx, rbx
0000000140cfee34  f30f114748                       movss dword ptr [rdi + 0x48], xmm0
0000000140cfee39  e8b292d701                       call 0x142a780f0
0000000140cfee3e  488bcb                           mov rcx, rbx
0000000140cfee41  f30f114750                       movss dword ptr [rdi + 0x50], xmm0
0000000140cfee46  e8a592d701                       call 0x142a780f0
0000000140cfee4b  488bcb                           mov rcx, rbx
0000000140cfee4e  f30f114754                       movss dword ptr [rdi + 0x54], xmm0
0000000140cfee53  e86892d701                       call 0x142a780c0
0000000140cfee58  488bcb                           mov rcx, rbx
0000000140cfee5b  88474c                           mov byte ptr [rdi + 0x4c], al
0000000140cfee5e  e8bd93d701                       call 0x142a78220
0000000140cfee63  488b5c2450                       mov rbx, qword ptr [rsp + 0x50]
0000000140cfee68  488b742458                       mov rsi, qword ptr [rsp + 0x58]
0000000140cfee6d  6689474e                         mov word ptr [rdi + 0x4e], ax
0000000140cfee71  4883c440                         add rsp, 0x40
0000000140cfee75  5f                               pop rdi
0000000140cfee76  c3                               ret
0000000140cfee77  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cfee7c  e8cfbcafff                       call 0x1407fab50
0000000140cfee81  488d1550821309                   lea rdx, [rip + 0x9138250]
0000000140cfee88  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cfee8d  e844216d03                       call 0x1443d0fd6
0000000140cfee92  cc                               int3
0000000140cfee93  cc                               int3
0000000140cfee94  cc                               int3
0000000140cfee95  cc                               int3
0000000140cfee96  cc                               int3
0000000140cfee97  cc                               int3
0000000140cfee98  cc                               int3
0000000140cfee99  cc                               int3

; prefix_B_public
0000000140cfeea0  48895c2408                       mov qword ptr [rsp + 8], rbx
0000000140cfeea5  4889742410                       mov qword ptr [rsp + 0x10], rsi
0000000140cfeeaa  57                               push rdi
0000000140cfeeab  4883ec40                         sub rsp, 0x40
0000000140cfeeaf  498bf1                           mov rsi, r9
0000000140cfeeb2  488bda                           mov rbx, rdx
0000000140cfeeb5  488bf9                           mov rdi, rcx
0000000140cfeeb8  664183f850                       cmp r8w, 0x50
0000000140cfeebd  755d                             jne 0x140cfef1c
0000000140cfeebf  488bca                           mov rcx, rdx
0000000140cfeec2  e8a993d701                       call 0x142a78270
0000000140cfeec7  83e801                           sub eax, 1
0000000140cfeeca  740c                             je 0x140cfeed8
0000000140cfeecc  83f801                           cmp eax, 1
0000000140cfeecf  7567                             jne 0x140cfef38
0000000140cfeed1  b802000000                       mov eax, 2
0000000140cfeed6  eb05                             jmp 0x140cfeedd
0000000140cfeed8  b801000000                       mov eax, 1
0000000140cfeedd  488bcb                           mov rcx, rbx
0000000140cfeee0  894708                           mov dword ptr [rdi + 8], eax
0000000140cfeee3  e88893d701                       call 0x142a78270
0000000140cfeee8  488d5710                         lea rdx, [rdi + 0x10]
0000000140cfeeec  89470c                           mov dword ptr [rdi + 0xc], eax
0000000140cfeeef  4c8bc6                           mov r8, rsi
0000000140cfeef2  488bcb                           mov rcx, rbx
0000000140cfeef5  e8f6cbffff                       call 0x140cfbaf0
0000000140cfeefa  488d5738                         lea rdx, [rdi + 0x38]
0000000140cfeefe  488bcb                           mov rcx, rbx
0000000140cfef01  4c8d8200010000                   lea r8, [rdx + 0x100]
0000000140cfef08  488b5c2450                       mov rbx, qword ptr [rsp + 0x50]
0000000140cfef0d  488b742458                       mov rsi, qword ptr [rsp + 0x58]
0000000140cfef12  4883c440                         add rsp, 0x40
0000000140cfef16  5f                               pop rdi
0000000140cfef17  e9a423ffff                       jmp 0x140cf12c0
0000000140cfef1c  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cfef21  e82abcafff                       call 0x1407fab50
0000000140cfef26  488d15ab811309                   lea rdx, [rip + 0x91381ab]
0000000140cfef2d  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cfef32  e89f206d03                       call 0x1443d0fd6
0000000140cfef37  cc                               int3
0000000140cfef38  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cfef3d  e87e85d8ff                       call 0x140a874c0
0000000140cfef42  488d1547861309                   lea rdx, [rip + 0x9138647]
0000000140cfef49  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cfef4e  e883206d03                       call 0x1443d0fd6
0000000140cfef53  cc                               int3
0000000140cfef54  cc                               int3
0000000140cfef55  cc                               int3
0000000140cfef56  cc                               int3
0000000140cfef57  cc                               int3
0000000140cfef58  cc                               int3
0000000140cfef59  cc                               int3
0000000140cfef5a  cc                               int3
0000000140cfef5b  cc                               int3
0000000140cfef5c  cc                               int3
0000000140cfef5d  cc                               int3
0000000140cfef5e  cc                               int3
0000000140cfef5f  cc                               int3
0000000140cfef60  48895c2408                       mov qword ptr [rsp + 8], rbx
0000000140cfef65  48896c2410                       mov qword ptr [rsp + 0x10], rbp
0000000140cfef6a  4889742418                       mov qword ptr [rsp + 0x18], rsi
0000000140cfef6f  48897c2420                       mov qword ptr [rsp + 0x20], rdi
0000000140cfef74  4154                             push r12
0000000140cfef76  4156                             push r14
0000000140cfef78  4157                             push r15
0000000140cfef7a  4883ec40                         sub rsp, 0x40
0000000140cfef7e  498be9                           mov rbp, r9
0000000140cfef81  4c8bf2                           mov r14, rdx
0000000140cfef84  664183f810                       cmp r8w, 0x10
0000000140cfef89  0f85b9000000                     jne 0x140cff048
0000000140cfef8f  496399c0020000                   movsxd rbx, dword ptr [r9 + 0x2c0]
0000000140cfef96  498b81b0020000                   mov rax, qword ptr [r9 + 0x2b0]
0000000140cfef9d  418b91bc020000                   mov edx, dword ptr [r9 + 0x2bc]
0000000140cfefa4  488b88f8d70200                   mov rcx, qword ptr [rax + 0x2d7f8]
0000000140cfefab  e83015c8ff                       call 0x1409804e0
0000000140cfefb0  488b8070130100                   mov rax, qword ptr [rax + 0x11370]
0000000140cfefb7  488b34d8                         mov rsi, qword ptr [rax + rbx*8]
0000000140cfefbb  0fb6be2c010000                   movzx edi, byte ptr [rsi + 0x12c]
0000000140cfefc2  0fb69e2d010000                   movzx ebx, byte ptr [rsi + 0x12d]
0000000140cfefc9  440fb7be1c010000                 movzx r15d, word ptr [rsi + 0x11c]

; wide_string_reader
0000000142a784b0  48895c2418                       mov qword ptr [rsp + 0x18], rbx
0000000142a784b5  55                               push rbp
0000000142a784b6  56                               push rsi
0000000142a784b7  57                               push rdi
0000000142a784b8  4154                             push r12
0000000142a784ba  4155                             push r13
0000000142a784bc  4156                             push r14
0000000142a784be  4157                             push r15
0000000142a784c0  4881ec70080000                   sub rsp, 0x870
0000000142a784c7  488b05f2d38207                   mov rax, qword ptr [rip + 0x782d3f2]
0000000142a784ce  4833c4                           xor rax, rsp
0000000142a784d1  4889842460080000                 mov qword ptr [rsp + 0x860], rax
0000000142a784d9  488bda                           mov rbx, rdx
0000000142a784dc  4c8be9                           mov r13, rcx
0000000142a784df  4889542438                       mov qword ptr [rsp + 0x38], rdx
0000000142a784e4  4533e4                           xor r12d, r12d
0000000142a784e7  488b4908                         mov rcx, qword ptr [rcx + 8]
0000000142a784eb  488b01                           mov rax, qword ptr [rcx]
0000000142a784ee  41b804000000                     mov r8d, 4
0000000142a784f4  488d542438                       lea rdx, [rsp + 0x38]
0000000142a784f9  ff5010                           call qword ptr [rax + 0x10]
0000000142a784fc  41837d1001                       cmp dword ptr [r13 + 0x10], 1
0000000142a78501  742d                             je 0x142a78530
0000000142a78503  8b442438                         mov eax, dword ptr [rsp + 0x38]
0000000142a78507  8bc8                             mov ecx, eax
0000000142a78509  c1e910                           shr ecx, 0x10
0000000142a7850c  0fb7d0                           movzx edx, ax
0000000142a7850f  c1ea08                           shr edx, 8
0000000142a78512  66c1e008                         shl ax, 8
0000000142a78516  0fb7c0                           movzx eax, ax
0000000142a78519  0bd0                             or edx, eax
0000000142a7851b  c1e210                           shl edx, 0x10
0000000142a7851e  8bc1                             mov eax, ecx
0000000142a78520  c1e808                           shr eax, 8
0000000142a78523  0bd0                             or edx, eax
0000000142a78525  66c1e108                         shl cx, 8
0000000142a78529  0fb7c1                           movzx eax, cx
0000000142a7852c  0bd0                             or edx, eax
0000000142a7852e  eb04                             jmp 0x142a78534
0000000142a78530  8b542438                         mov edx, dword ptr [rsp + 0x38]
0000000142a78534  85d2                             test edx, edx
0000000142a78536  7451                             je 0x142a78589
0000000142a78538  8bfa                             mov edi, edx
0000000142a7853a  4c8d343f                         lea r14, [rdi + rdi]
0000000142a7853e  81fa00040000                     cmp edx, 0x400
0000000142a78544  0f8381000000                     jae 0x142a785cb
0000000142a7854a  498b4d08                         mov rcx, qword ptr [r13 + 8]
0000000142a7854e  488b01                           mov rax, qword ptr [rcx]
0000000142a78551  4d8bc6                           mov r8, r14
0000000142a78554  488d542460                       lea rdx, [rsp + 0x60]
0000000142a78559  ff5010                           call qword ptr [rax + 0x10]
0000000142a7855c  488d4c2460                       lea rcx, [rsp + 0x60]
0000000142a78561  41837d1001                       cmp dword ptr [r13 + 0x10], 1
0000000142a78566  741c                             je 0x142a78584
0000000142a78568  4885ff                           test rdi, rdi
0000000142a7856b  7417                             je 0x142a78584
0000000142a7856d  0f1f00                           nop dword ptr [rax]
0000000142a78570  0fb701                           movzx eax, word ptr [rcx]
0000000142a78573  66c1c808                         ror ax, 8
0000000142a78577  668901                           mov word ptr [rcx], ax
0000000142a7857a  488d4902                         lea rcx, [rcx + 2]
0000000142a7857e  4883ef01                         sub rdi, 1
0000000142a78582  75ec                             jne 0x142a78570
0000000142a78584  49d1fe                           sar r14, 1
0000000142a78587  751a                             jne 0x142a785a3
0000000142a78589  0f57c0                           xorps xmm0, xmm0
0000000142a7858c  0f1103                           movups xmmword ptr [rbx], xmm0
0000000142a7858f  4c896310                         mov qword ptr [rbx + 0x10], r12
0000000142a78593  48c743180f000000                 mov qword ptr [rbx + 0x18], 0xf
0000000142a7859b  448823                           mov byte ptr [rbx], r12b
0000000142a7859e  e96e010000                       jmp 0x142a78711
0000000142a785a3  4c8d442460                       lea r8, [rsp + 0x60]
0000000142a785a8  4f8d0470                         lea r8, [r8 + r14*2]
0000000142a785ac  488d442430                       lea rax, [rsp + 0x30]
0000000142a785b1  4889442420                       mov qword ptr [rsp + 0x20], rax
0000000142a785b6  4533c9                           xor r9d, r9d
0000000142a785b9  488d542460                       lea rdx, [rsp + 0x60]
0000000142a785be  488bcb                           mov rcx, rbx
0000000142a785c1  e86a2ec8fd                       call 0x1406fb430
0000000142a785c6  e946010000                       jmp 0x142a78711
0000000142a785cb  0f57c0                           xorps xmm0, xmm0
0000000142a785ce  f30f7f442440                     movdqu xmmword ptr [rsp + 0x40], xmm0
0000000142a785d4  498bec                           mov rbp, r12
0000000142a785d7  4c89642450                       mov qword ptr [rsp + 0x50], r12
0000000142a785dc  85d2                             test edx, edx
0000000142a785de  746c                             je 0x142a7864c
0000000142a785e0  4d85f6                           test r14, r14
0000000142a785e3  7505                             jne 0x142a785ea
0000000142a785e5  498bf4                           mov rsi, r12
0000000142a785e8  eb3d                             jmp 0x142a78627
0000000142a785ea  4981fe00100000                   cmp r14, 0x1000
0000000142a785f1  7229                             jb 0x142a7861c
0000000142a785f3  498d4e27                         lea rcx, [r14 + 0x27]
0000000142a785f7  493bce                           cmp rcx, r14
0000000142a785fa  0f863f010000                     jbe 0x142a7873f
0000000142a78600  e88f0b9501                       call 0x1443c9194
0000000142a78605  4885c0                           test rax, rax
0000000142a78608  0f84f1000000                     je 0x142a786ff
0000000142a7860e  488d7027                         lea rsi, [rax + 0x27]
0000000142a78612  4883e6e0                         and rsi, 0xffffffffffffffe0
0000000142a78616  488946f8                         mov qword ptr [rsi - 8], rax
0000000142a7861a  eb0b                             jmp 0x142a78627
0000000142a7861c  498bce                           mov rcx, r14
0000000142a7861f  e8700b9501                       call 0x1443c9194
0000000142a78624  488bf0                           mov rsi, rax
0000000142a78627  4889742440                       mov qword ptr [rsp + 0x40], rsi
0000000142a7862c  498d2c36                         lea rbp, [r14 + rsi]
0000000142a78630  48896c2450                       mov qword ptr [rsp + 0x50], rbp
0000000142a78635  4d8bc6                           mov r8, r14
0000000142a78638  33d2                             xor edx, edx
0000000142a7863a  488bce                           mov rcx, rsi
0000000142a7863d  e8be899501                       call 0x1443d1000
0000000142a78642  4c8bfd                           mov r15, rbp
0000000142a78645  48896c2448                       mov qword ptr [rsp + 0x48], rbp
0000000142a7864a  eb0a                             jmp 0x142a78656
0000000142a7864c  4c8b7c2448                       mov r15, qword ptr [rsp + 0x48]
0000000142a78651  488b742440                       mov rsi, qword ptr [rsp + 0x40]
0000000142a78656  498b4d08                         mov rcx, qword ptr [r13 + 8]
0000000142a7865a  488b01                           mov rax, qword ptr [rcx]
0000000142a7865d  4d8bc6                           mov r8, r14
0000000142a78660  488bd6                           mov rdx, rsi
0000000142a78663  ff5010                           call qword ptr [rax + 0x10]
0000000142a78666  488bce                           mov rcx, rsi
0000000142a78669  41837d1001                       cmp dword ptr [r13 + 0x10], 1
0000000142a7866e  7424                             je 0x142a78694
0000000142a78670  4885ff                           test rdi, rdi
0000000142a78673  741f                             je 0x142a78694
0000000142a78675  6666660f1f840000000000           nop word ptr [rax + rax]
0000000142a78680  0fb701                           movzx eax, word ptr [rcx]
0000000142a78683  66c1c808                         ror ax, 8
0000000142a78687  668901                           mov word ptr [rcx], ax
0000000142a7868a  488d4902                         lea rcx, [rcx + 2]
0000000142a7868e  4883ef01                         sub rdi, 1
0000000142a78692  75ec                             jne 0x142a78680
0000000142a78694  4c2bfe                           sub r15, rsi
0000000142a78697  49d1ff                           sar r15, 1
0000000142a7869a  7517                             jne 0x142a786b3
0000000142a7869c  0f57c0                           xorps xmm0, xmm0
0000000142a7869f  0f1103                           movups xmmword ptr [rbx], xmm0
0000000142a786a2  4c896310                         mov qword ptr [rbx + 0x10], r12
0000000142a786a6  48c743180f000000                 mov qword ptr [rbx + 0x18], 0xf
0000000142a786ae  c60300                           mov byte ptr [rbx], 0
0000000142a786b1  eb1d                             jmp 0x142a786d0
0000000142a786b3  4e8d047e                         lea r8, [rsi + r15*2]
0000000142a786b7  488d442430                       lea rax, [rsp + 0x30]
0000000142a786bc  4889442420                       mov qword ptr [rsp + 0x20], rax
0000000142a786c1  4533c9                           xor r9d, r9d
0000000142a786c4  488bd6                           mov rdx, rsi
0000000142a786c7  488bcb                           mov rcx, rbx
0000000142a786ca  e8612dc8fd                       call 0x1406fb430
0000000142a786cf  90                               nop
0000000142a786d0  4885f6                           test rsi, rsi
0000000142a786d3  743c                             je 0x142a78711
0000000142a786d5  482bee                           sub rbp, rsi
0000000142a786d8  48d1fd                           sar rbp, 1
0000000142a786db  4803ed                           add rbp, rbp
0000000142a786de  488bc6                           mov rax, rsi
0000000142a786e1  4881fd00100000                   cmp rbp, 0x1000
0000000142a786e8  721c                             jb 0x142a78706
0000000142a786ea  4883c527                         add rbp, 0x27
0000000142a786ee  488b76f8                         mov rsi, qword ptr [rsi - 8]
0000000142a786f2  482bc6                           sub rax, rsi
0000000142a786f5  4883c0f8                         add rax, -8
0000000142a786f9  4883f81f                         cmp rax, 0x1f
0000000142a786fd  7607                             jbe 0x142a78706
0000000142a786ff  ff159b89c701                     call qword ptr [rip + 0x1c7899b]
0000000142a78705  cc                               int3
0000000142a78706  488bd5                           mov rdx, rbp
0000000142a78709  488bce                           mov rcx, rsi
0000000142a7870c  e8bf0a9501                       call 0x1443c91d0
0000000142a78711  488bc3                           mov rax, rbx
0000000142a78714  488b8c2460080000                 mov rcx, qword ptr [rsp + 0x860]
0000000142a7871c  4833cc                           xor rcx, rsp
0000000142a7871f  e86c0e9501                       call 0x1443c9590
0000000142a78724  488b9c24c0080000                 mov rbx, qword ptr [rsp + 0x8c0]
0000000142a7872c  4881c470080000                   add rsp, 0x870
0000000142a78733  415f                             pop r15
0000000142a78735  415e                             pop r14
0000000142a78737  415d                             pop r13
0000000142a78739  415c                             pop r12
0000000142a7873b  5f                               pop rdi
0000000142a7873c  5e                               pop rsi
0000000142a7873d  5d                               pop rbp
0000000142a7873e  c3                               ret
0000000142a7873f  e8fceba9fd                       call 0x140517340
0000000142a78744  cc                               int3
0000000142a78745  cc                               int3
0000000142a78746  cc                               int3
0000000142a78747  cc                               int3
0000000142a78748  cc                               int3

; automation_array_modern_gate
0000000140cf14f0  48895c2408                       mov qword ptr [rsp + 8], rbx
0000000140cf14f5  4889742410                       mov qword ptr [rsp + 0x10], rsi
0000000140cf14fa  57                               push rdi
0000000140cf14fb  4883ec20                         sub rsp, 0x20
0000000140cf14ff  498bf8                           mov rdi, r8
0000000140cf1502  488bf2                           mov rsi, rdx
0000000140cf1505  488bd9                           mov rbx, rcx
0000000140cf1508  e8636dd801                       call 0x142a78270
0000000140cf150d  85c0                             test eax, eax
0000000140cf150f  740e                             je 0x140cf151f
0000000140cf1511  4c8bc7                           mov r8, rdi
0000000140cf1514  488bd6                           mov rdx, rsi

; automation_array_modern_records
0000000140cf08b0  48895c2408                       mov qword ptr [rsp + 8], rbx
0000000140cf08b5  4889742410                       mov qword ptr [rsp + 0x10], rsi
0000000140cf08ba  48897c2418                       mov qword ptr [rsp + 0x18], rdi
0000000140cf08bf  55                               push rbp
0000000140cf08c0  4154                             push r12
0000000140cf08c2  4155                             push r13
0000000140cf08c4  4156                             push r14
0000000140cf08c6  4157                             push r15
0000000140cf08c8  488d6c24c9                       lea rbp, [rsp - 0x37]
0000000140cf08cd  4881eca0000000                   sub rsp, 0xa0
0000000140cf08d4  4d8bf8                           mov r15, r8
0000000140cf08d7  4c8bf2                           mov r14, rdx
0000000140cf08da  488bf1                           mov rsi, rcx
0000000140cf08dd  e88e79d801                       call 0x142a78270
0000000140cf08e2  448be0                           mov r12d, eax
0000000140cf08e5  33ff                             xor edi, edi
0000000140cf08e7  85c0                             test eax, eax
0000000140cf08e9  0f846f010000                     je 0x140cf0a5e
0000000140cf08ef  4c8d2d3274a603                   lea r13, [rip + 0x3a67432]
0000000140cf08f6  488d4dd7                         lea rcx, [rbp - 0x29]
0000000140cf08fa  e8a11fb0ff                       call 0x1407f28a0
0000000140cf08ff  90                               nop
0000000140cf0900  488d4dd7                         lea rcx, [rbp - 0x29]
0000000140cf0904  488b45d7                         mov rax, qword ptr [rbp - 0x29]
0000000140cf0908  ff5030                           call qword ptr [rax + 0x30]
0000000140cf090b  488bce                           mov rcx, rsi
0000000140cf090e  e8ad77d801                       call 0x142a780c0
0000000140cf0913  0fb6d8                           movzx ebx, al
0000000140cf0916  488d4dd7                         lea rcx, [rbp - 0x29]
0000000140cf091a  488b55d7                         mov rdx, qword ptr [rbp - 0x29]
0000000140cf091e  ff5218                           call qword ptr [rdx + 0x18]
0000000140cf0921  3ad8                             cmp bl, al
0000000140cf0923  0f8572010000                     jne 0x140cf0a9b
0000000140cf0929  488bce                           mov rcx, rsi
0000000140cf092c  e8ef78d801                       call 0x142a78220
0000000140cf0931  0fb7d8                           movzx ebx, ax
0000000140cf0934  488d4dd7                         lea rcx, [rbp - 0x29]
0000000140cf0938  488b55d7                         mov rdx, qword ptr [rbp - 0x29]
0000000140cf093c  ff5210                           call qword ptr [rdx + 0x10]
0000000140cf093f  663bd8                           cmp bx, ax
0000000140cf0942  0f8739010000                     ja 0x140cf0a81
0000000140cf0948  488b4dd7                         mov rcx, qword ptr [rbp - 0x29]
0000000140cf094c  4c8b5140                         mov r10, qword ptr [rcx + 0x40]
0000000140cf0950  4d8bcf                           mov r9, r15
0000000140cf0953  440fb7c3                         movzx r8d, bx
0000000140cf0957  488bd6                           mov rdx, rsi
0000000140cf095a  488d4dd7                         lea rcx, [rbp - 0x29]
0000000140cf095e  41ffd2                           call r10
0000000140cf0961  488b4608                         mov rax, qword ptr [rsi + 8]
0000000140cf0965  4885c0                           test rax, rax
0000000140cf0968  0f8447010000                     je 0x140cf0ab5
0000000140cf096e  8b400c                           mov eax, dword ptr [rax + 0xc]
0000000140cf0971  83f803                           cmp eax, 3
0000000140cf0974  7408                             je 0x140cf097e
0000000140cf0976  85c0                             test eax, eax
0000000140cf0978  7404                             je 0x140cf097e
0000000140cf097a  32c0                             xor al, al
0000000140cf097c  eb02                             jmp 0x140cf0980
0000000140cf097e  b001                             mov al, 1
0000000140cf0980  84c0                             test al, al
0000000140cf0982  0f852d010000                     jne 0x140cf0ab5
0000000140cf0988  488b45d7                         mov rax, qword ptr [rbp - 0x29]
0000000140cf098c  4d8bc7                           mov r8, r15
0000000140cf098f  0fb7d3                           movzx edx, bx
0000000140cf0992  488d4dd7                         lea rcx, [rbp - 0x29]
0000000140cf0996  ff5038                           call qword ptr [rax + 0x38]
0000000140cf0999  498b5608                         mov rdx, qword ptr [r14 + 8]
0000000140cf099d  493b5610                         cmp rdx, qword ptr [r14 + 0x10]
0000000140cf09a1  0f849f000000                     je 0x140cf0a46
0000000140cf09a7  4c892a                           mov qword ptr [rdx], r13
0000000140cf09aa  0fb645df                         movzx eax, byte ptr [rbp - 0x21]
0000000140cf09ae  884208                           mov byte ptr [rdx + 8], al
0000000140cf09b1  0fb645e0                         movzx eax, byte ptr [rbp - 0x20]
0000000140cf09b5  884209                           mov byte ptr [rdx + 9], al
0000000140cf09b8  f30f1045e3                       movss xmm0, dword ptr [rbp - 0x1d]
0000000140cf09bd  f30f11420c                       movss dword ptr [rdx + 0xc], xmm0
0000000140cf09c2  488b45e7                         mov rax, qword ptr [rbp - 0x19]
0000000140cf09c6  48894210                         mov qword ptr [rdx + 0x10], rax
0000000140cf09ca  8b45ff                           mov eax, dword ptr [rbp - 1]
0000000140cf09cd  894228                           mov dword ptr [rdx + 0x28], eax
0000000140cf09d0  f30f104503                       movss xmm0, dword ptr [rbp + 3]
0000000140cf09d5  f30f11422c                       movss dword ptr [rdx + 0x2c], xmm0
0000000140cf09da  0fb64507                         movzx eax, byte ptr [rbp + 7]
0000000140cf09de  884230                           mov byte ptr [rdx + 0x30], al
0000000140cf09e1  0fb64508                         movzx eax, byte ptr [rbp + 8]
0000000140cf09e5  884231                           mov byte ptr [rdx + 0x31], al
0000000140cf09e8  0fb74509                         movzx eax, word ptr [rbp + 9]
0000000140cf09ec  66894232                         mov word ptr [rdx + 0x32], ax
0000000140cf09f0  8b450b                           mov eax, dword ptr [rbp + 0xb]
0000000140cf09f3  894234                           mov dword ptr [rdx + 0x34], eax
0000000140cf09f6  8b450f                           mov eax, dword ptr [rbp + 0xf]
0000000140cf09f9  894238                           mov dword ptr [rdx + 0x38], eax
0000000140cf09fc  8b4513                           mov eax, dword ptr [rbp + 0x13]
0000000140cf09ff  89423c                           mov dword ptr [rdx + 0x3c], eax
0000000140cf0a02  8b4517                           mov eax, dword ptr [rbp + 0x17]
0000000140cf0a05  894240                           mov dword ptr [rdx + 0x40], eax
0000000140cf0a08  8b451b                           mov eax, dword ptr [rbp + 0x1b]
0000000140cf0a0b  894244                           mov dword ptr [rdx + 0x44], eax
0000000140cf0a0e  488b451f                         mov rax, qword ptr [rbp + 0x1f]
0000000140cf0a12  48894248                         mov qword ptr [rdx + 0x48], rax
0000000140cf0a16  f30f104527                       movss xmm0, dword ptr [rbp + 0x27]
0000000140cf0a1b  f30f114250                       movss dword ptr [rdx + 0x50], xmm0
0000000140cf0a20  f30f104d2b                       movss xmm1, dword ptr [rbp + 0x2b]
0000000140cf0a25  f30f114a54                       movss dword ptr [rdx + 0x54], xmm1
0000000140cf0a2a  8b452f                           mov eax, dword ptr [rbp + 0x2f]
0000000140cf0a2d  894258                           mov dword ptr [rdx + 0x58], eax
0000000140cf0a30  0fb64533                         movzx eax, byte ptr [rbp + 0x33]
0000000140cf0a34  88425c                           mov byte ptr [rdx + 0x5c], al
0000000140cf0a37  0fb74535                         movzx eax, word ptr [rbp + 0x35]
0000000140cf0a3b  6689425e                         mov word ptr [rdx + 0x5e], ax
0000000140cf0a3f  4983460860                       add qword ptr [r14 + 8], 0x60
0000000140cf0a44  eb0d                             jmp 0x140cf0a53
0000000140cf0a46  4c8d45d7                         lea r8, [rbp - 0x29]
0000000140cf0a4a  498bce                           mov rcx, r14
0000000140cf0a4d  e8ee5f9aff                       call 0x140696a40
0000000140cf0a52  90                               nop
0000000140cf0a53  ffc7                             inc edi
0000000140cf0a55  413bfc                           cmp edi, r12d
0000000140cf0a58  0f8298feffff                     jb 0x140cf08f6
0000000140cf0a5e  8bc7                             mov eax, edi
0000000140cf0a60  4c8d9c24a0000000                 lea r11, [rsp + 0xa0]
0000000140cf0a68  498b5b30                         mov rbx, qword ptr [r11 + 0x30]
0000000140cf0a6c  498b7338                         mov rsi, qword ptr [r11 + 0x38]
0000000140cf0a70  498b7b40                         mov rdi, qword ptr [r11 + 0x40]
0000000140cf0a74  498be3                           mov rsp, r11
0000000140cf0a77  415f                             pop r15
0000000140cf0a79  415e                             pop r14
0000000140cf0a7b  415d                             pop r13
0000000140cf0a7d  415c                             pop r12
0000000140cf0a7f  5d                               pop rbp
0000000140cf0a80  c3                               ret
0000000140cf0a81  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0a85  e8d664b0ff                       call 0x1407f6f60
0000000140cf0a8a  488d15f7641409                   lea rdx, [rip + 0x91464f7]
0000000140cf0a91  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0a95  e83c056e03                       call 0x1443d0fd6
0000000140cf0a9a  cc                               int3
0000000140cf0a9b  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0a9f  e89ca1b0ff                       call 0x1407fac40
0000000140cf0aa4  488d1545641409                   lea rdx, [rip + 0x9146445]
0000000140cf0aab  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0aaf  e822056e03                       call 0x1443d0fd6
0000000140cf0ab4  cc                               int3
0000000140cf0ab5  488d4db7                         lea rcx, [rbp - 0x49]
0000000140cf0ab9  e8129fb0ff                       call 0x1407fa9d0
0000000140cf0abe  488d1533651409                   lea rdx, [rip + 0x9146533]

; automation_array_legacy
0000000140cefcf0  48896c2420                       mov qword ptr [rsp + 0x20], rbp
0000000140cefcf5  56                               push rsi
0000000140cefcf6  57                               push rdi
0000000140cefcf7  4156                             push r14
0000000140cefcf9  4883ec40                         sub rsp, 0x40
0000000140cefcfd  4d8bf0                           mov r14, r8
0000000140cefd00  488bea                           mov rbp, rdx
0000000140cefd03  488bf1                           mov rsi, rcx
0000000140cefd06  e86585d801                       call 0x142a78270
0000000140cefd0b  8bd0                             mov edx, eax
0000000140cefd0d  488bcd                           mov rcx, rbp
0000000140cefd10  8bf8                             mov edi, eax
0000000140cefd12  e889c6a4ff                       call 0x14073c3a0
0000000140cefd17  85ff                             test edi, edi
0000000140cefd19  0f84ed000000                     je 0x140cefe0c
0000000140cefd1f  8b5534                           mov edx, dword ptr [rbp + 0x34]
0000000140cefd22  b960000000                       mov ecx, 0x60
0000000140cefd27  48895c2460                       mov qword ptr [rsp + 0x60], rbx
0000000140cefd2c  85d2                             test edx, edx
0000000140cefd2e  488b5d08                         mov rbx, qword ptr [rbp + 8]
0000000140cefd32  0f4fca                           cmovg ecx, edx
0000000140cefd35  4c89642468                       mov qword ptr [rsp + 0x68], r12
0000000140cefd3a  0fafcf                           imul ecx, edi
0000000140cefd3d  4c897c2470                       mov qword ptr [rsp + 0x70], r15
0000000140cefd42  4c63f9                           movsxd r15, ecx
0000000140cefd45  488bce                           mov rcx, rsi
0000000140cefd48  4c03fb                           add r15, rbx
0000000140cefd4b  e82085d801                       call 0x142a78270
0000000140cefd50  33ed                             xor ebp, ebp
0000000140cefd52  448be0                           mov r12d, eax
0000000140cefd55  85c0                             test eax, eax
0000000140cefd57  0f84a0000000                     je 0x140cefdfd
0000000140cefd5d  0f1f00                           nop dword ptr [rax]
0000000140cefd60  493bdf                           cmp rbx, r15
0000000140cefd63  0f8494000000                     je 0x140cefdfd
0000000140cefd69  488b13                           mov rdx, qword ptr [rbx]
0000000140cefd6c  488bcb                           mov rcx, rbx
0000000140cefd6f  ff5230                           call qword ptr [rdx + 0x30]
0000000140cefd72  488bce                           mov rcx, rsi
0000000140cefd75  e84683d801                       call 0x142a780c0
0000000140cefd7a  488b13                           mov rdx, qword ptr [rbx]
0000000140cefd7d  488bcb                           mov rcx, rbx
0000000140cefd80  0fb6f8                           movzx edi, al
0000000140cefd83  ff5218                           call qword ptr [rdx + 0x18]
0000000140cefd86  403af8                           cmp dil, al
0000000140cefd89  0f858b000000                     jne 0x140cefe1a
0000000140cefd8f  488bce                           mov rcx, rsi
0000000140cefd92  e88984d801                       call 0x142a78220
0000000140cefd97  488b13                           mov rdx, qword ptr [rbx]
0000000140cefd9a  488bcb                           mov rcx, rbx
0000000140cefd9d  0fb7f8                           movzx edi, ax
0000000140cefda0  ff5210                           call qword ptr [rdx + 0x10]
0000000140cefda3  663bf8                           cmp di, ax
0000000140cefda6  0f87a6000000                     ja 0x140cefe52
0000000140cefdac  4c8b13                           mov r10, qword ptr [rbx]
0000000140cefdaf  4d8bce                           mov r9, r14
0000000140cefdb2  440fb7c7                         movzx r8d, di
0000000140cefdb6  488bd6                           mov rdx, rsi
0000000140cefdb9  488bcb                           mov rcx, rbx
0000000140cefdbc  41ff5240                         call qword ptr [r10 + 0x40]
0000000140cefdc0  488b4608                         mov rax, qword ptr [rsi + 8]
0000000140cefdc4  4885c0                           test rax, rax
0000000140cefdc7  746d                             je 0x140cefe36
0000000140cefdc9  8b400c                           mov eax, dword ptr [rax + 0xc]
0000000140cefdcc  83f803                           cmp eax, 3
0000000140cefdcf  7408                             je 0x140cefdd9
0000000140cefdd1  85c0                             test eax, eax
0000000140cefdd3  7404                             je 0x140cefdd9
0000000140cefdd5  32c0                             xor al, al
0000000140cefdd7  eb02                             jmp 0x140cefddb
0000000140cefdd9  b001                             mov al, 1
0000000140cefddb  84c0                             test al, al
0000000140cefddd  7557                             jne 0x140cefe36
0000000140cefddf  488b03                           mov rax, qword ptr [rbx]
0000000140cefde2  4d8bc6                           mov r8, r14
0000000140cefde5  0fb7d7                           movzx edx, di
0000000140cefde8  488bcb                           mov rcx, rbx
0000000140cefdeb  ff5038                           call qword ptr [rax + 0x38]
0000000140cefdee  ffc5                             inc ebp
0000000140cefdf0  4883c360                         add rbx, 0x60
0000000140cefdf4  413bec                           cmp ebp, r12d
0000000140cefdf7  0f8263ffffff                     jb 0x140cefd60
0000000140cefdfd  4c8b642468                       mov r12, qword ptr [rsp + 0x68]
0000000140cefe02  488b5c2460                       mov rbx, qword ptr [rsp + 0x60]
0000000140cefe07  4c8b7c2470                       mov r15, qword ptr [rsp + 0x70]
0000000140cefe0c  488b6c2478                       mov rbp, qword ptr [rsp + 0x78]
0000000140cefe11  4883c440                         add rsp, 0x40
0000000140cefe15  415e                             pop r14
0000000140cefe17  5f                               pop rdi
0000000140cefe18  5e                               pop rsi
0000000140cefe19  c3                               ret
0000000140cefe1a  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cefe1f  e81caeb0ff                       call 0x1407fac40
0000000140cefe24  488d15c5701409                   lea rdx, [rip + 0x91470c5]
0000000140cefe2b  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cefe30  e8a1116e03                       call 0x1443d0fd6
0000000140cefe35  cc                               int3
0000000140cefe36  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cefe3b  e890abb0ff                       call 0x1407fa9d0
0000000140cefe40  488d15b1711409                   lea rdx, [rip + 0x91471b1]

; automation_array_writer
0000000140cf25a0  48895c2420                       mov qword ptr [rsp + 0x20], rbx
0000000140cf25a5  55                               push rbp
0000000140cf25a6  57                               push rdi
0000000140cf25a7  4156                             push r14
0000000140cf25a9  4883ec40                         sub rsp, 0x40
0000000140cf25ad  4c8b4a08                         mov r9, qword ptr [rdx + 8]
0000000140cf25b1  4c8bf2                           mov r14, rdx
0000000140cf25b4  4c2b0a                           sub r9, qword ptr [rdx]
0000000140cf25b7  48b8abaaaaaaaaaaaa2a             movabs rax, 0x2aaaaaaaaaaaaaab
0000000140cf25c1  49f7e9                           imul r9
0000000140cf25c4  498be8                           mov rbp, r8
0000000140cf25c7  488bf9                           mov rdi, rcx
0000000140cf25ca  488bda                           mov rbx, rdx
0000000140cf25cd  48c1fb04                         sar rbx, 4
0000000140cf25d1  488bc3                           mov rax, rbx
0000000140cf25d4  48c1e83f                         shr rax, 0x3f
0000000140cf25d8  4803d8                           add rbx, rax
0000000140cf25db  8bd3                             mov edx, ebx
0000000140cf25dd  e8ceb7d801                       call 0x142a7ddb0
0000000140cf25e2  85db                             test ebx, ebx
0000000140cf25e4  0f8426010000                     je 0x140cf2710
0000000140cf25ea  488b4f08                         mov rcx, qword ptr [rdi + 8]
0000000140cf25ee  498b1e                           mov rbx, qword ptr [r14]
0000000140cf25f1  4d8b7608                         mov r14, qword ptr [r14 + 8]
0000000140cf25f5  4889742460                       mov qword ptr [rsp + 0x60], rsi
0000000140cf25fa  4c897c2470                       mov qword ptr [rsp + 0x70], r15
0000000140cf25ff  41bfffffffff                     mov r15d, 0xffffffff
0000000140cf2605  4c89642468                       mov qword ptr [rsp + 0x68], r12
0000000140cf260a  4885c9                           test rcx, rcx
0000000140cf260d  740b                             je 0x140cf261a
0000000140cf260f  488b01                           mov rax, qword ptr [rcx]
0000000140cf2612  ff5020                           call qword ptr [rax + 0x20]
0000000140cf2615  448be0                           mov r12d, eax
0000000140cf2618  eb03                             jmp 0x140cf261d
0000000140cf261a  458be7                           mov r12d, r15d
0000000140cf261d  33d2                             xor edx, edx
0000000140cf261f  488bcf                           mov rcx, rdi
0000000140cf2622  e889b7d801                       call 0x142a7ddb0
0000000140cf2627  33f6                             xor esi, esi
0000000140cf2629  493bde                           cmp rbx, r14
0000000140cf262c  0f84cf000000                     je 0x140cf2701
0000000140cf2632  488b03                           mov rax, qword ptr [rbx]
0000000140cf2635  488bd5                           mov rdx, rbp
0000000140cf2638  488bcb                           mov rcx, rbx
0000000140cf263b  ff5020                           call qword ptr [rax + 0x20]
0000000140cf263e  488b03                           mov rax, qword ptr [rbx]
0000000140cf2641  488bcb                           mov rcx, rbx
0000000140cf2644  ff5018                           call qword ptr [rax + 0x18]
0000000140cf2647  0fb6d0                           movzx edx, al
0000000140cf264a  488bcf                           mov rcx, rdi
0000000140cf264d  e84eb5d801                       call 0x142a7dba0
0000000140cf2652  488b03                           mov rax, qword ptr [rbx]
0000000140cf2655  488bcb                           mov rcx, rbx
0000000140cf2658  ff5010                           call qword ptr [rax + 0x10]
0000000140cf265b  0fb7d0                           movzx edx, ax
0000000140cf265e  488bcf                           mov rcx, rdi
0000000140cf2661  e80ab7d801                       call 0x142a7dd70
0000000140cf2666  488b03                           mov rax, qword ptr [rbx]
0000000140cf2669  4c8bc5                           mov r8, rbp
0000000140cf266c  488bd7                           mov rdx, rdi
0000000140cf266f  488bcb                           mov rcx, rbx
0000000140cf2672  ff5048                           call qword ptr [rax + 0x48]
0000000140cf2675  488b4708                         mov rax, qword ptr [rdi + 8]
0000000140cf2679  4885c0                           test rax, rax
0000000140cf267c  0f849c000000                     je 0x140cf271e
0000000140cf2682  8b400c                           mov eax, dword ptr [rax + 0xc]
0000000140cf2685  83f803                           cmp eax, 3
0000000140cf2688  7408                             je 0x140cf2692
0000000140cf268a  85c0                             test eax, eax
0000000140cf268c  7404                             je 0x140cf2692
0000000140cf268e  32c0                             xor al, al
0000000140cf2690  eb02                             jmp 0x140cf2694
0000000140cf2692  b001                             mov al, 1
0000000140cf2694  84c0                             test al, al
0000000140cf2696  0f8582000000                     jne 0x140cf271e
0000000140cf269c  488b03                           mov rax, qword ptr [rbx]
0000000140cf269f  488bd5                           mov rdx, rbp
0000000140cf26a2  488bcb                           mov rcx, rbx
0000000140cf26a5  ff5028                           call qword ptr [rax + 0x28]
0000000140cf26a8  ffc6                             inc esi
0000000140cf26aa  4883c360                         add rbx, 0x60
0000000140cf26ae  493bde                           cmp rbx, r14
0000000140cf26b1  0f857bffffff                     jne 0x140cf2632
0000000140cf26b7  85f6                             test esi, esi
0000000140cf26b9  7446                             je 0x140cf2701
0000000140cf26bb  488b4f08                         mov rcx, qword ptr [rdi + 8]
0000000140cf26bf  4885c9                           test rcx, rcx
0000000140cf26c2  7409                             je 0x140cf26cd
0000000140cf26c4  488b01                           mov rax, qword ptr [rcx]
0000000140cf26c7  ff5020                           call qword ptr [rax + 0x20]
0000000140cf26ca  448bf8                           mov r15d, eax
0000000140cf26cd  488b4f08                         mov rcx, qword ptr [rdi + 8]
0000000140cf26d1  4885c9                           test rcx, rcx
0000000140cf26d4  740c                             je 0x140cf26e2
0000000140cf26d6  488b01                           mov rax, qword ptr [rcx]
0000000140cf26d9  4533c0                           xor r8d, r8d
0000000140cf26dc  418bd4                           mov edx, r12d
0000000140cf26df  ff5030                           call qword ptr [rax + 0x30]
0000000140cf26e2  8bd6                             mov edx, esi
0000000140cf26e4  488bcf                           mov rcx, rdi
0000000140cf26e7  e8c4b6d801                       call 0x142a7ddb0
0000000140cf26ec  488b4f08                         mov rcx, qword ptr [rdi + 8]
0000000140cf26f0  4885c9                           test rcx, rcx
0000000140cf26f3  740c                             je 0x140cf2701
0000000140cf26f5  488b01                           mov rax, qword ptr [rcx]
0000000140cf26f8  4533c0                           xor r8d, r8d
0000000140cf26fb  418bd7                           mov edx, r15d
0000000140cf26fe  ff5030                           call qword ptr [rax + 0x30]
0000000140cf2701  4c8b642468                       mov r12, qword ptr [rsp + 0x68]
0000000140cf2706  488b742460                       mov rsi, qword ptr [rsp + 0x60]
0000000140cf270b  4c8b7c2470                       mov r15, qword ptr [rsp + 0x70]
0000000140cf2710  488b5c2478                       mov rbx, qword ptr [rsp + 0x78]
0000000140cf2715  4883c440                         add rsp, 0x40
0000000140cf2719  415e                             pop r14
0000000140cf271b  5f                               pop rdi
0000000140cf271c  5d                               pop rbp
0000000140cf271d  c3                               ret
0000000140cf271e  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cf2723  e86883b0ff                       call 0x1407faa90
0000000140cf2728  488d1539491409                   lea rdx, [rip + 0x9144939]
0000000140cf272f  488d4c2420                       lea rcx, [rsp + 0x20]
0000000140cf2734  e89de86d03                       call 0x1443d0fd6
0000000140cf2739  cc                               int3
0000000140cf273a  cc                               int3
0000000140cf273b  cc                               int3
0000000140cf273c  cc                               int3
0000000140cf273d  cc                               int3
0000000140cf273e  cc                               int3
0000000140cf273f  cc                               int3
0000000140cf2740  4883ec28                         sub rsp, 0x28
0000000140cf2744  488b02                           mov rax, qword ptr [rdx]
0000000140cf2747  48b9aaaaaaaaaaaaaa02             movabs rcx, 0x2aaaaaaaaaaaaaa
0000000140cf2751  483bc1                           cmp rax, rcx
0000000140cf2754  7757                             ja 0x140cf27ad

; slider40_tag_pair
00000001409b695c  c78424a40c0000db170000           mov dword ptr [rsp + 0xca4], 0x17db
00000001409b6967  4c8d8424a40c0000                 lea r8, [rsp + 0xca4]
00000001409b696f  488d1582864e04                   lea rdx, [rip + 0x44e8682]
00000001409b6976  488d8c24c06a0100                 lea rcx, [rsp + 0x16ac0]
00000001409b697e  e84d63e0ff                       call 0x1407bccd0

; slider_tag_ordinal
0000000140993fc0  448d814de8ffff                   lea r8d, [rcx - 0x17b3]
0000000140993fc7  4181f882130000                   cmp r8d, 0x1382
0000000140993fce  773b                             ja 0x14099400b
0000000140993fd0  85d2                             test edx, edx
0000000140993fd2  7433                             je 0x140994007
0000000140993fd4  83ea01                           sub edx, 1
0000000140993fd7  7427                             je 0x140994000
0000000140993fd9  83ea01                           sub edx, 1
0000000140993fdc  741b                             je 0x140993ff9
0000000140993fde  83ea01                           sub edx, 1
0000000140993fe1  740f                             je 0x140993ff2
0000000140993fe3  83fa01                           cmp edx, 1
0000000140993fe6  7403                             je 0x140993feb
0000000140993fe8  33c0                             xor eax, eax
0000000140993fea  c3                               ret
0000000140993feb  8d81b1d8ffff                     lea eax, [rcx - 0x274f]
0000000140993ff1  c3                               ret
0000000140993ff2  8d8198dcffff                     lea eax, [rcx - 0x2368]
0000000140993ff8  c3                               ret
0000000140993ff9  8d817fe0ffff                     lea eax, [rcx - 0x1f81]
0000000140993fff  c3                               ret
0000000140994000  8d8166e4ffff                     lea eax, [rcx - 0x1b9a]
0000000140994006  c3                               ret
0000000140994007  418bc0                           mov eax, r8d
000000014099400a  c3                               ret
000000014099400b  b8ffffffff                       mov eax, 0xffffffff
0000000140994010  c3                               ret
0000000140994011  cc                               int3
0000000140994012  cc                               int3
0000000140994013  cc                               int3
0000000140994014  cc                               int3
0000000140994015  cc                               int3
0000000140994016  cc                               int3
0000000140994017  cc                               int3
0000000140994018  cc                               int3
0000000140994019  cc                               int3
000000014099401a  cc                               int3
000000014099401b  cc                               int3
000000014099401c  cc                               int3
000000014099401d  cc                               int3
000000014099401e  cc                               int3
000000014099401f  cc                               int3

; typed_control_lookup
00000001407a3ba0  48895c2408                       mov qword ptr [rsp + 8], rbx
00000001407a3ba5  4889742410                       mov qword ptr [rsp + 0x10], rsi
00000001407a3baa  57                               push rdi
00000001407a3bab  4883ec20                         sub rsp, 0x20
00000001407a3baf  4963d8                           movsxd rbx, r8d
00000001407a3bb2  8bf2                             mov esi, edx
00000001407a3bb4  448bc3                           mov r8d, ebx
00000001407a3bb7  488bf9                           mov rdi, rcx
00000001407a3bba  e8210a0000                       call 0x1407a45e0
00000001407a3bbf  84c0                             test al, al
00000001407a3bc1  7512                             jne 0x1407a3bd5
00000001407a3bc3  33c0                             xor eax, eax
00000001407a3bc5  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3bca  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3bcf  4883c420                         add rsp, 0x20
00000001407a3bd3  5f                               pop rdi
00000001407a3bd4  c3                               ret
00000001407a3bd5  33d2                             xor edx, edx
00000001407a3bd7  8d46ff                           lea eax, [rsi - 1]
00000001407a3bda  83f80f                           cmp eax, 0xf
00000001407a3bdd  0f8714020000                     ja 0x1407a3df7
00000001407a3be3  4c8d0516c485ff                   lea r8, [rip - 0x7a3bea]
00000001407a3bea  4898                             cdqe
00000001407a3bec  418b8c800c3e7a00                 mov ecx, dword ptr [r8 + rax*4 + 0x7a3e0c]
00000001407a3bf4  4903c8                           add rcx, r8
00000001407a3bf7  ffe1                             jmp rcx
00000001407a3bf9  4869d3f0010000                   imul rdx, rbx, 0x1f0
00000001407a3c00  480397a8840000                   add rdx, qword ptr [rdi + 0x84a8]
00000001407a3c07  488bc2                           mov rax, rdx
00000001407a3c0a  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3c0f  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3c14  4883c420                         add rsp, 0x20
00000001407a3c18  5f                               pop rdi
00000001407a3c19  c3                               ret
00000001407a3c1a  4869d3b0010000                   imul rdx, rbx, 0x1b0
00000001407a3c21  48039700870000                   add rdx, qword ptr [rdi + 0x8700]
00000001407a3c28  488bc2                           mov rax, rdx
00000001407a3c2b  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3c30  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3c35  4883c420                         add rsp, 0x20
00000001407a3c39  5f                               pop rdi
00000001407a3c3a  c3                               ret
00000001407a3c3b  4869d328020000                   imul rdx, rbx, 0x228
00000001407a3c42  48039760860000                   add rdx, qword ptr [rdi + 0x8660]
00000001407a3c49  488bc2                           mov rax, rdx
00000001407a3c4c  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3c51  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3c56  4883c420                         add rsp, 0x20
00000001407a3c5a  5f                               pop rdi
00000001407a3c5b  c3                               ret
00000001407a3c5c  4869d3f8010000                   imul rdx, rbx, 0x1f8
00000001407a3c63  48039738860000                   add rdx, qword ptr [rdi + 0x8638]
00000001407a3c6a  488bc2                           mov rax, rdx
00000001407a3c6d  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3c72  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3c77  4883c420                         add rsp, 0x20
00000001407a3c7b  5f                               pop rdi
00000001407a3c7c  c3                               ret
00000001407a3c7d  488d145b                         lea rdx, [rbx + rbx*2]
00000001407a3c81  48c1e209                         shl rdx, 9
00000001407a3c85  48039798850000                   add rdx, qword ptr [rdi + 0x8598]
00000001407a3c8c  488bc2                           mov rax, rdx
00000001407a3c8f  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3c94  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3c99  4883c420                         add rsp, 0x20
00000001407a3c9d  5f                               pop rdi
00000001407a3c9e  c3                               ret
00000001407a3c9f  4869d3d8010000                   imul rdx, rbx, 0x1d8
00000001407a3ca6  480397c0850000                   add rdx, qword ptr [rdi + 0x85c0]
00000001407a3cad  488bc2                           mov rax, rdx
00000001407a3cb0  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3cb5  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3cba  4883c420                         add rsp, 0x20
00000001407a3cbe  5f                               pop rdi
00000001407a3cbf  c3                               ret
00000001407a3cc0  4869d320020000                   imul rdx, rbx, 0x220
00000001407a3cc7  480397d0840000                   add rdx, qword ptr [rdi + 0x84d0]
00000001407a3cce  488bc2                           mov rax, rdx
00000001407a3cd1  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3cd6  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3cdb  4883c420                         add rsp, 0x20
00000001407a3cdf  5f                               pop rdi
00000001407a3ce0  c3                               ret
00000001407a3ce1  4869d308020000                   imul rdx, rbx, 0x208
00000001407a3ce8  480397f8840000                   add rdx, qword ptr [rdi + 0x84f8]
00000001407a3cef  488bc2                           mov rax, rdx
00000001407a3cf2  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3cf7  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3cfc  4883c420                         add rsp, 0x20
00000001407a3d00  5f                               pop rdi
00000001407a3d01  c3                               ret
00000001407a3d02  4869d3e8010000                   imul rdx, rbx, 0x1e8
00000001407a3d09  48039720850000                   add rdx, qword ptr [rdi + 0x8520]
00000001407a3d10  488bc2                           mov rax, rdx
00000001407a3d13  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3d18  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3d1d  4883c420                         add rsp, 0x20
00000001407a3d21  5f                               pop rdi
00000001407a3d22  c3                               ret
00000001407a3d23  4869d3f8010000                   imul rdx, rbx, 0x1f8
00000001407a3d2a  48039748850000                   add rdx, qword ptr [rdi + 0x8548]
00000001407a3d31  488bc2                           mov rax, rdx
00000001407a3d34  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3d39  488b742438                       mov rsi, qword ptr [rsp + 0x38]
00000001407a3d3e  4883c420                         add rsp, 0x20
00000001407a3d42  5f                               pop rdi
00000001407a3d43  c3                               ret
00000001407a3d44  4869d3e0010000                   imul rdx, rbx, 0x1e0
00000001407a3d4b  48039788860000                   add rdx, qword ptr [rdi + 0x8688]
00000001407a3d52  488bc2                           mov rax, rdx
00000001407a3d55  488b5c2430                       mov rbx, qword ptr [rsp + 0x30]
00000001407a3d5a  488b742438                       mov rsi, qword ptr [rsp + 0x38]

; automation_creation_address_gate VA 0x1408f3479 (23 bytes)
0x1408f3479  b801080000                        mov eax, 0x801
0x1408f347e  440fb79d38010000                  movzx r11d, word ptr [rbp + 0x138]
0x1408f3486  66443bd8                          cmp r11w, ax
0x1408f348a  0f8313090000                      jae 0x1408f3da3

; automation_host_address_remap VA 0x1409ab730 (432 bytes)
0x1409ab730  48896c2418                        mov qword ptr [rsp + 0x18], rbp
0x1409ab735  4889742420                        mov qword ptr [rsp + 0x20], rsi
0x1409ab73a  4156                              push r14
0x1409ab73c  8ba930cf0100                      mov ebp, dword ptr [rcx + 0x1cf30]
0x1409ab742  488bf1                            mov rsi, rcx
0x1409ab745  2ba934cf0100                      sub ebp, dword ptr [rcx + 0x1cf34]
0x1409ab74b  83b908cf0100ff                    cmp dword ptr [rcx + 0x1cf08], -1
0x1409ab752  7409                              je 0x1409ab75d
0x1409ab754  448b910ccf0100                    mov r10d, dword ptr [rcx + 0x1cf0c]
0x1409ab75b  eb1d                              jmp 0x1409ab77a
0x1409ab75d  8b8114cf0100                      mov eax, dword ptr [rcx + 0x1cf14]
0x1409ab763  85c0                              test eax, eax
0x1409ab765  b960000000                        mov ecx, 0x60
0x1409ab76a  0f4fc8                            cmovg ecx, eax
0x1409ab76d  8b86f0ce0100                      mov eax, dword ptr [rsi + 0x1cef0]
0x1409ab773  33d2                              xor edx, edx
0x1409ab775  f7f1                              div ecx
0x1409ab777  448bd0                            mov r10d, eax
0x1409ab77a  4533c9                            xor r9d, r9d
0x1409ab77d  4585d2                            test r10d, r10d
0x1409ab780  7e61                              jle 0x1409ab7e3
0x1409ab782  0f1f4000                          nop dword ptr [rax]
0x1409ab786  66660f1f840000000000              nop word ptr [rax + rax]
0x1409ab790  8b8614cf0100                      mov eax, dword ptr [rsi + 0x1cf14]
0x1409ab796  b960000000                        mov ecx, 0x60
0x1409ab79b  85c0                              test eax, eax
0x1409ab79d  0f4fc8                            cmovg ecx, eax
0x1409ab7a0  410fafc9                          imul ecx, r9d
0x1409ab7a4  4c63c1                            movsxd r8, ecx
0x1409ab7a7  4c0386e8ce0100                    add r8, qword ptr [rsi + 0x1cee8]
0x1409ab7ae  4183782802                        cmp dword ptr [r8 + 0x28], 2
0x1409ab7b3  7526                              jne 0x1409ab7db
0x1409ab7b5  4180780800                        cmp byte ptr [r8 + 8], 0
0x1409ab7ba  751f                              jne 0x1409ab7db
0x1409ab7bc  8b15be946204                      mov edx, dword ptr [rip + 0x46294be]
0x1409ab7c2  33c0                              xor eax, eax
0x1409ab7c4  410fb74832                        movzx ecx, word ptr [r8 + 0x32]
0x1409ab7c9  ffca                              dec edx
0x1409ab7cb  03cd                              add ecx, ebp
0x1409ab7cd  0f49c1                            cmovns eax, ecx
0x1409ab7d0  3bc2                              cmp eax, edx
0x1409ab7d2  660f4ed0                          cmovle dx, ax
0x1409ab7d6  6641895032                        mov word ptr [r8 + 0x32], dx
0x1409ab7db  41ffc1                            inc r9d
0x1409ab7de  453bca                            cmp r9d, r10d
0x1409ab7e1  7cad                              jl 0x1409ab790
0x1409ab7e3  488b9670130100                    mov rdx, qword ptr [rsi + 0x11370]
0x1409ab7ea  488b8678130100                    mov rax, qword ptr [rsi + 0x11378]
0x1409ab7f1  482bc2                            sub rax, rdx
0x1409ab7f4  48c1f803                          sar rax, 3
0x1409ab7f8  4c63f0                            movsxd r14, eax
0x1409ab7fb  85c0                              test eax, eax
0x1409ab7fd  0f8ed0000000                      jle 0x1409ab8d3
0x1409ab803  48895c2410                        mov qword ptr [rsp + 0x10], rbx
0x1409ab808  48897c2418                        mov qword ptr [rsp + 0x18], rdi
0x1409ab80d  33ff                              xor edi, edi
0x1409ab80f  90                                nop
0x1409ab810  4c8b14fa                          mov r10, qword ptr [rdx + rdi*8]
0x1409ab814  4183ba08010000ff                  cmp dword ptr [r10 + 0x108], -1
0x1409ab81c  7409                              je 0x1409ab827
0x1409ab81e  458b9a0c010000                    mov r11d, dword ptr [r10 + 0x10c]
0x1409ab825  eb26                              jmp 0x1409ab84d
0x1409ab827  418b8214010000                    mov eax, dword ptr [r10 + 0x114]
0x1409ab82e  b960000000                        mov ecx, 0x60
0x1409ab833  85c0                              test eax, eax
0x1409ab835  0f4fc8                            cmovg ecx, eax
0x1409ab838  418b82f0000000                    mov eax, dword ptr [r10 + 0xf0]
0x1409ab83f  33d2                              xor edx, edx
0x1409ab841  f7f1                              div ecx
0x1409ab843  488b9670130100                    mov rdx, qword ptr [rsi + 0x11370]
0x1409ab84a  448bd8                            mov r11d, eax
0x1409ab84d  33c9                              xor ecx, ecx
0x1409ab84f  4585db                            test r11d, r11d
0x1409ab852  7e69                              jle 0x1409ab8bd
0x1409ab854  0f1f4000                          nop dword ptr [rax]
0x1409ab858  0f1f840000000000                  nop dword ptr [rax + rax]
0x1409ab860  418b8214010000                    mov eax, dword ptr [r10 + 0x114]
0x1409ab867  ba60000000                        mov edx, 0x60
0x1409ab86c  85c0                              test eax, eax
0x1409ab86e  0f4fd0                            cmovg edx, eax
0x1409ab871  0fafd1                            imul edx, ecx
0x1409ab874  4c63ca                            movsxd r9, edx
0x1409ab877  4d038ae8000000                    add r9, qword ptr [r10 + 0xe8]
0x1409ab87e  4183792802                        cmp dword ptr [r9 + 0x28], 2
0x1409ab883  752a                              jne 0x1409ab8af
0x1409ab885  4180790800                        cmp byte ptr [r9 + 8], 0
0x1409ab88a  7523                              jne 0x1409ab8af
0x1409ab88c  448b05ed936204                    mov r8d, dword ptr [rip + 0x46293ed]
0x1409ab893  33c0                              xor eax, eax
0x1409ab895  410fb75132                        movzx edx, word ptr [r9 + 0x32]
0x1409ab89a  41ffc8                            dec r8d
0x1409ab89d  03d5                              add edx, ebp
0x1409ab89f  0f49c2                            cmovns eax, edx
0x1409ab8a2  413bc0                            cmp eax, r8d
0x1409ab8a5  66440f4ec0                        cmovle r8w, ax
0x1409ab8aa  6645894132                        mov word ptr [r9 + 0x32], r8w
0x1409ab8af  ffc1                              inc ecx
0x1409ab8b1  413bcb                            cmp ecx, r11d
0x1409ab8b4  7caa                              jl 0x1409ab860
0x1409ab8b6  488b9670130100                    mov rdx, qword ptr [rsi + 0x11370]
0x1409ab8bd  48ffc7                            inc rdi
0x1409ab8c0  493bfe                            cmp rdi, r14
0x1409ab8c3  0f8c47ffffff                      jl 0x1409ab810
0x1409ab8c9  488b7c2418                        mov rdi, qword ptr [rsp + 0x18]
0x1409ab8ce  488b5c2410                        mov rbx, qword ptr [rsp + 0x10]
0x1409ab8d3  488b6c2420                        mov rbp, qword ptr [rsp + 0x20]
0x1409ab8d8  488b742428                        mov rsi, qword ptr [rsp + 0x28]
0x1409ab8dd  415e                              pop r14
0x1409ab8df  c3                                ret

; automation_host_limit_initial VA 0x144fd4c80 (4 bytes)
0x144fd4c80  01080000  .long 2049
