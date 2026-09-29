# Resource recovery checks

The read-only index audit covered 268 NKX/NKR archives. All 951,741 unavailable members have 22 zero bytes where a member header should be. The remaining member headers passed the index checks; sample payload decoding was not audited in full.

For each affected archive, sampled a 64 KiB window around its first, middle and last invalid member offset (clipped at file boundaries). 582 of 600 sampled windows were entirely zero. These larger zero-filled regions cannot be reconstructed from an encryption key.

Searched valid entries in sibling archives inside each library for an exact case-insensitive member-path match. No candidates were found. Also searched the following folders for duplicate damaged archive filenames and library-named ZIP/RAR/7z/ISO files; no additional candidates were found:

- `/mnt/MAIN_STORAGE/Installers`
- `/mnt/MAIN_STORAGE/Libraries`
- `/mnt/MAIN_STORAGE/Samples`
- `/home/derpcat/Downloads`

Compressed backup contents and other locations were not exhaustively searched. Existing library files were not rewritten. A replacement must supply the actual missing bytes; guessed sample substitution is not a repair.

| Library | Unavailable members | Sibling candidates | Entirely zero sampled windows |
|---|---:|---:|---:|
| Afflatus Chapter II Brass | 270751 | 0 | 166/171 |
| Areia 1.2.0 [Audio Imperia] | 212941 | 0 | 115/117 |
| Audio Imperia CHORUS | 161888 | 0 | 96/99 |
| Audio Imperia Dolce | 241301 | 0 | 108/114 |
| Pacific Ensemble Strings | 59398 | 0 | 79/81 |
| Performance Samples Vista | 0 | 0 | 0/0 |
| Solo | 0 | 0 | 0/0 |
| Una Corda Library | 5462 | 0 | 18/18 |
