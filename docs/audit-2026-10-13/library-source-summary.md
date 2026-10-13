# Offline inspector catalog evidence

Catalog record coverage/schema complete: **true**. Static conversion admission: **complete_static_catalog**. Native equivalence: **unknown**.

Record completeness means each manifest ID has one structurally valid report; it does not require its compiler, source frontend, IR, or UI stages to succeed.
Static conversion admission additionally requires complete static source inspection, every expected UVI XML-to-IR conversion, and no integrity faults.

Manifest items: 1495; observed: 1495; missing: 0; unexpected: 0.
Static source inspected: 1495; source semantics without reported gaps: 0; Static IR converted: 660; static UI projected: 0.
UVI compiler-only syntax: 780 ok, 0 failed. This is Luau syntax evidence, not native Falcon Lua behavior.
Separate initialized samples: 17 attempted, 17 IR, 9 UI, 8 incomplete.
Runtime/audio/native comparison requires independent evidence and is not inferred here.

| Library | Expected | Observed | Missing | Source inspected | Kontakt files: all programs | Static IR | Init IR | Init UI | Scripts | Constructs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Kontakt/ANALOG STRINGS | 1 | 1 | 0 | 1 | 1 | 0 | 1 | 1 | 5 | 12027 |
| Kontakt/Afflatus Chapter II Brass | 348 | 348 | 0 | 348 | 348 | 0 | 1 | 1 | 1740 | 1052175 |
| Kontakt/Areia 1.2.0 [Audio Imperia] | 155 | 155 | 0 | 155 | 155 | 0 | 1 | 1 | 775 | 48607928 |
| Kontakt/Audio Imperia CHORUS | 42 | 42 | 0 | 42 | 42 | 0 | 1 | 1 | 240 | 8671497 |
| Kontakt/Audio Imperia Dolce | 77 | 77 | 0 | 77 | 77 | 0 | 1 | 1 | 385 | 21186396 |
| Kontakt/Conflux 1.1.0 [Native Instruments] | 51 | 51 | 0 | 51 | 51 | 0 | 3 | 0 | 505 | 453393 |
| Kontakt/Morphology Evolved [Zero-G] rutracker.org | 1 | 1 | 0 | 1 | 1 | 0 | 1 | 0 | 5 | 39615 |
| Kontakt/Pacific Ensemble Strings | 50 | 50 | 0 | 50 | 50 | 0 | 1 | 1 | 265 | 80925 |
| Kontakt/Performance Samples Vista | 7 | 7 | 0 | 7 | 7 | 0 | 1 | 1 | 35 | 9188 |
| Kontakt/Solo | 100 | 100 | 0 | 100 | 100 | 0 | 1 | 1 | 500 | 14486002 |
| Kontakt/Una Corda Library | 3 | 3 | 0 | 3 | 3 | 0 | 1 | 1 | 15 | 17001 |
| UVI/UVI - Augmented Orchestra v1.1.2-R2R | 620 | 620 | 0 | 620 | 0 | 620 | 1 | 0 | 620 | 620 |
| UVI/VWinds - Clarinets | 16 | 16 | 0 | 16 | 0 | 16 | 1 | 0 | 64 | 17394 |
| UVI/VWinds - Double Reeds | 15 | 15 | 0 | 15 | 0 | 15 | 1 | 0 | 60 | 15641 |
| UVI/VWinds - Flutes | 9 | 9 | 0 | 9 | 0 | 9 | 1 | 0 | 36 | 10098 |

## Provenance

- manifest `all-items-current.tsv`: SHA-256 `02be1aa7cc78c41557e700628b106b95fa7ade3e2cccbc29af61309175158092` (226574 bytes)
- report_jsonl `static-00.jsonl`: SHA-256 `7a2bcd71b94d6015d2be8110563b0300c4aa0b2f599ad09c2ac7a3ed44020663` (93186 bytes)
- report_jsonl `static-01.jsonl`: SHA-256 `96b85c1bf99611a02a3a6b7452c361b6b101107f36d20a54865a3ba6c1a7540a` (93962 bytes)
- report_jsonl `static-02.jsonl`: SHA-256 `ffe3c96cd5ad7341dc6ab023f55704ca4d9bf48f00f80cbc8f8f39cae3ded021` (94019 bytes)
- report_jsonl `static-03.jsonl`: SHA-256 `7a47d94ca843f460e1b28cbdd5dcd56fbf4bfd319fd192e35a405da22bf479c0` (93912 bytes)
- report_jsonl `static-04.jsonl`: SHA-256 `eb52b6540574276147ca0aa5aec7f00a78032ddba428c724fc5c554481951030` (94111 bytes)
- report_jsonl `static-05.jsonl`: SHA-256 `0c524e2f28ec3590737e0f9bf8073a5ac5d89acc9f8b27f55a5835324c0da1c7` (94223 bytes)
- report_jsonl `static-06.jsonl`: SHA-256 `ddac273357d76e4ae2b198f1da45e3c520618af8cfa1c33227b0d258dd16e456` (94904 bytes)
- report_jsonl `static-07.jsonl`: SHA-256 `2bb6faf41a3485b77cd660aee6bc0086239148c01f3368fe8d8d5bb64ac9b2ee` (94210 bytes)
- report_jsonl `static-08.jsonl`: SHA-256 `0303baa05b57ffc8c2c87358e8742931ff0d996dc977637034bb9724b4ac25bf` (94654 bytes)
- report_jsonl `static-09.jsonl`: SHA-256 `ca24c9c8ade1cfca834a8b9fe2e8bad55a8c7abac953553fd25510c27bc9e31c` (95009 bytes)
- report_jsonl `static-10.jsonl`: SHA-256 `1a1c118b1c2c0da2bf9a19293c06f664875a69fa5cf62489519eac59ae00304d` (94537 bytes)
- report_jsonl `static-11.jsonl`: SHA-256 `7f696633b90b8c61a1b52f2ba0d915bcaa40ca8c22ce40beff2d672137a9e752` (93843 bytes)
- report_jsonl `static-12.jsonl`: SHA-256 `0cccc95a84b3e9e0fff84fc376bffb07cd9416707175040e4770f64cd3f19487` (93674 bytes)
- report_jsonl `static-13.jsonl`: SHA-256 `1e8c645d260cd1373cf3059fdbf844f1d8b8454b9e084a0a1a919d529bf20349` (94365 bytes)
- report_jsonl `static-14.jsonl`: SHA-256 `3d5d47819b3c76c457910c3cb5559e8ac2d8a076ca6d0c5be53fa2931a39295f` (84338 bytes)
- report_jsonl `static-15.jsonl`: SHA-256 `71fae526ae2a4f6511373082e62dad6bcb3545583a6a6a768e3502ebd5c43efe` (84514 bytes)
- report_jsonl `static-16.jsonl`: SHA-256 `fb29dd582d7eb9b9034fb49d4a4f3f2848cb8ea08f616d1a5d13094474c7c8c7` (84519 bytes)
- report_jsonl `static-17.jsonl`: SHA-256 `7115fba1c851613707ca36bfe32ec163fa778b7f099de261d1f13a89b5a0b8c8` (85081 bytes)
- report_jsonl `static-18.jsonl`: SHA-256 `ff9c128c87b0efaff95371d191246cf513110e140995bb85c9b64d16d12bde0d` (84930 bytes)
- report_jsonl `static-19.jsonl`: SHA-256 `9975e6885a4ca0389b926055814e6c6a54fc0fe37da69e21e099ff42575679c4` (84336 bytes)
- report_jsonl `static-20.jsonl`: SHA-256 `c31c9287518cacde1d81984087e3134bbf881fa9dce1fa5749275c8247b42cee` (84615 bytes)
- report_jsonl `static-21.jsonl`: SHA-256 `d7fc78d0f3a54a588314b2fcd712d44a79679823575e64aa3f87b158bff1d50d` (94660 bytes)
- report_jsonl `static-22.jsonl`: SHA-256 `07345bb6f878c2ea505ae0c04834c91b4f2b5f2d02de38dfd386860120f85935` (83484 bytes)
- report_jsonl `static-23.jsonl`: SHA-256 `4f21901870c408d9835249b0ca2fa8eba2b959968c0120fe8b939d09dbcd1749` (83072 bytes)
- report_jsonl `static-24.jsonl`: SHA-256 `99dce773b88ccee723466aebdd7c91959d7a7fd6da648e6512a9f83e6aa9a177` (104994 bytes)
- report_jsonl `static-25.jsonl`: SHA-256 `f24249584474f457dc6e2d2ed094061bb67caded5f829bf69f968c7339eeedda` (391613 bytes)
- report_jsonl `static-26.jsonl`: SHA-256 `84eb840c0b860e0c3f4d4250180a8794800c900ff4d6114a63ec139c48d6cd4c` (369701 bytes)
- report_jsonl `static-27.jsonl`: SHA-256 `d9e93f44b812815564efb46879c4e9bb78e6a6aee99a229ba351e40b0f5bf9e8` (83791 bytes)
- report_jsonl `static-28.jsonl`: SHA-256 `35f6774a8f87f2a3c2190ef90b241b5765146c5f9ef0acd8e75081383908f7eb` (89391 bytes)
- report_jsonl `static-29.jsonl`: SHA-256 `34c9011b1d167037f6ded169887ca78b4fcf44ba8fdb043a9ea973eacf1c395f` (82570 bytes)
- report_jsonl `static-30.jsonl`: SHA-256 `7fdafad8317336ebcbbf944cefb9320f36b72c450782f6adf7662633e37bf36f` (83444 bytes)
- report_jsonl `static-31.jsonl`: SHA-256 `486b0317c457403f97e0aa4121773aa175a1b1c0cc75339b36802fe5aedcc722` (83741 bytes)
- report_jsonl `static-32.jsonl`: SHA-256 `a260f125e73d48b3ac765180e2512c3c0ef72f2775ea8f30b2637f9699c084e8` (83753 bytes)
- report_jsonl `static-33.jsonl`: SHA-256 `8e108d8795c96c7898d7f5887110baca079d562845945a41c3093d71ea7ad79b` (11963123 bytes)
- report_jsonl `static-34.jsonl`: SHA-256 `b2cfbc729c7862bf2ef3f43c590ca22e407cec815a08bcf88b18fe4bb4a23fa9` (19879403 bytes)
- report_jsonl `static-35.jsonl`: SHA-256 `c5a16e30c11711b07897177d643ead460449ce652adceabb42a197313e964b8a` (19879893 bytes)
- report_jsonl `static-36.jsonl`: SHA-256 `956818cfea28ac971bb8a57064de990515ef7a1f39b683088fa00e9bb488a1e0` (19878822 bytes)
- report_jsonl `static-37.jsonl`: SHA-256 `1d8d379682cea095eee36b346338885150ae1f8c9d3f718db4d385f60542c372` (19879814 bytes)
- report_jsonl `static-38.jsonl`: SHA-256 `222cdbc904c76c42242dd3bc4479d4b0170219a71072287acac339691daa2f13` (21242822 bytes)
- report_jsonl `static-39.jsonl`: SHA-256 `83d7035251a4cb5517cc2381582f91194760a3a5f23f5ed93eaf7f8ab4d21f71` (21867874 bytes)
- report_jsonl `static-40.jsonl`: SHA-256 `065ddd1061d1d682d7d543a8d07d4183fa661069b51e1f45d0e0aee745ea0efd` (20899127 bytes)
- report_jsonl `static-41.jsonl`: SHA-256 `bea1b1e9ddb3115dd8438397c503654eaa4086302a8f6953dc1cf8853986e0dd` (23047376 bytes)
- report_jsonl `static-42.jsonl`: SHA-256 `60d1d3e685c4a5b9da3bf02373bccc22050432625add613891dbbc62fd528eec` (24103102 bytes)
- report_jsonl `static-43.jsonl`: SHA-256 `b17835cbe8f348666b3c977ec2d064e7ca7efb214bf2a449287c3d9bd65ac3d2` (34739402 bytes)
- report_jsonl `static-44.jsonl`: SHA-256 `b53ebcfc202dace8f7c818f2c680d5a81505b1e5cd1d407c59480e041c021312` (24581604 bytes)
- report_jsonl `static-45.jsonl`: SHA-256 `68d18016da52e7f95c0d76229c3f5b85c798fdffe44c24d7b16be5ab301632b7` (24969209 bytes)
- report_jsonl `static-46.jsonl`: SHA-256 `b1618afed8bf1be5790cebfec8393cf35d79d752fac2e70a3082dde35a65c2b3` (28175131 bytes)
- report_jsonl `static-47.jsonl`: SHA-256 `e6d39f7775582c54b0a2972f3df2f252808fed5f69c4b4561bb74793e3b23a96` (30235863 bytes)
- report_jsonl `static-48.jsonl`: SHA-256 `77629b0a47e9667806b03349c262c268c69f7cb92c52daf2bc7a55d7c6184e59` (29896266 bytes)
- report_jsonl `static-49.jsonl`: SHA-256 `f5bd9e553998ae3d20d1b695827d5300c1d8891fedc10b3c58222eb137aa7ea0` (20504129 bytes)
- report_jsonl `static-50.jsonl`: SHA-256 `4f44c76d796e362ac2fdfef692a0eacd5d49250ddcd3f56ee7c7877912b460a7` (21877604 bytes)
- report_jsonl `static-51.jsonl`: SHA-256 `adcd7da55f0fd5187b104dec6e5dfb9de05740a5c73794abf96558d53a73db95` (19880072 bytes)
- report_jsonl `static-52.jsonl`: SHA-256 `c4142ab5cbd917a07a8e3c8ce55ec5ba54d739d5c3016e0bef52baa4bf0d6e92` (19880556 bytes)
- report_jsonl `static-53.jsonl`: SHA-256 `be558c9b1ca2bfbfa32d7b2da8cca4a583b8dc371a01b9521cf2f65dd7b98c44` (19877857 bytes)
- report_jsonl `static-54.jsonl`: SHA-256 `1fbbcddcaaf67886fd60e8041de7be377433fec6a6edf62caf7232f0e17de41b` (29758003 bytes)
- report_jsonl `static-55.jsonl`: SHA-256 `9d6380c875a3e86930431368b6290d2a15e417928a9d40e7443ca33407d1904b` (26621346 bytes)
- report_jsonl `static-56.jsonl`: SHA-256 `807e27ac950d68c577cad7846e98f8eb145e4a0a79f558f4256a7e3bf84682ce` (28410763 bytes)
- report_jsonl `static-57.jsonl`: SHA-256 `29a0c3724e447023e0992208eeed2bf816efe657a7bd76aba3542f8fd576af12` (27333840 bytes)
- report_jsonl `static-58.jsonl`: SHA-256 `9f4753bc74bf1f24afebf363d35ba77e5f15e6dcb77017c9ef9747033f86836d` (8552243 bytes)
- report_jsonl `static-59.jsonl`: SHA-256 `609f4e1b086523f4202e6b8fca5b7e2ae997dc7c1be2e109a012dbb406e7ad52` (3988531 bytes)
- initialized_report_jsonl `initialized-00.jsonl`: SHA-256 `503a754aaeb77752699a24bb551e6e647f3c85eda8e4f667d3e5dd6e7876557e` (103528172 bytes)
- initialized_report_jsonl `initialized-01.jsonl`: SHA-256 `71940015005e6560ce3267b8d3535491a20f07d1dc7b8685aa6c7f55f49cf2b2` (15241917 bytes)
- initialized_report_jsonl `initialized-02.jsonl`: SHA-256 `da7d5dcccd580563ad88f05d5f1ef2ecf52b4505abd394ac64cc779bb35d8e36` (370064541 bytes)
- initialized_report_jsonl `initialized-03.jsonl`: SHA-256 `34ad9ab69e5a9e929eee156e401d72eb7f4bf6aec7b87543f425eb5f5ed95f71` (216293927 bytes)
- initialized_report_jsonl `initialized-04.jsonl`: SHA-256 `b618b3b0f5659ff9e01a5acb8edda740fa318ede9a664748be8a3fd8532d00d0` (296034203 bytes)
- initialized_report_jsonl `initialized-05.jsonl`: SHA-256 `77db02d86ccca49674fbec4853241e1c77b2a3ad5fa372998dac04bafc4a8ff7` (13917066 bytes)
- initialized_report_jsonl `initialized-06.jsonl`: SHA-256 `680a43c83d0f84cc4797d61cb62dfea7295530badd15384adcb736216fcfcfa3` (14764016 bytes)
- initialized_report_jsonl `initialized-07.jsonl`: SHA-256 `32f0820977cf1083f6d6083dd6c321b09073453670b301cc67c7417b16733523` (49169999 bytes)
- initialized_report_jsonl `initialized-08.jsonl`: SHA-256 `908b9b6899169b1573674f7aa8b33a235b922f682b1d80c3b817e2136c8ca0e3` (5925927 bytes)
- initialized_report_jsonl `initialized-09.jsonl`: SHA-256 `502a0720be74646b443563e18ec2bf10ba72adad09f9cbbf4af27b017f334323` (9012175 bytes)
- initialized_report_jsonl `initialized-10.jsonl`: SHA-256 `043bbc1f61c93f117628d9d615a8dfe9a54398cec6fc22dd5a85cd9fd01dba19` (161059440 bytes)
- initialized_report_jsonl `initialized-11.jsonl`: SHA-256 `8ac8da25e9abb9c520a4a2740585a2297f280c99605ea05934827a41f5ececd8` (13240148 bytes)
- initialized_report_jsonl `initialized-12.jsonl`: SHA-256 `a8b11b8d072b07ea9e4d941300ed6488a36c8b1f6a2723806e2e26d627cf8b9b` (35813131 bytes)
- initialized_report_jsonl `initialized-13.jsonl`: SHA-256 `011408e6a73ee60a40b87545604112fc514e7ca53bf1a35eb25b6c417ed22063` (6465330 bytes)
- initialized_report_jsonl `initialized-14.jsonl`: SHA-256 `f7b40e042ef6984239393e880bdee0b3968d02fd6a9b67c213f7e078f01f6670` (6175928 bytes)
- initialized_report_jsonl `initialized-15.jsonl`: SHA-256 `c2dd207273b9a11209be4ff9278891301e1ed86d7326d1f62f39ed9c5907adf1` (7626509 bytes)
- initialized_report_jsonl `initialized-16.jsonl`: SHA-256 `e376d1a159feebb18a8b19eb4b81c217a1367801cfc2663428314c662ecc029a` (19458220 bytes)

## Integrity

No manifest, identity, or report integrity faults detected.
