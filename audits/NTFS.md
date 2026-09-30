# Zero reads on /mnt/MAIN_STORAGE (kernel `ntfs` driver)

Status: needs root once. No userspace workaround exists (see Ruled out).

## Cause

`/mnt/MAIN_STORAGE` is `/dev/nvme0n1p1` mounted with the in-kernel `ntfs` driver (Linux 7.3.0-rc4-cachyos, `errors=continue`). Large files written by Windows over hours (interleaved downloads) are fragmented enough that their `$DATA` runlist continues in extension MFT records through `$ATTRIBUTE_LIST`. The driver's attribute validator (`ntfs_attr_value_is_valid()` in `fs/ntfs/attrib.c`, added by the 2026 lookup-hardening commits d5803e3345da, ffc755828de1 and a83e82b0ec3a) rejects every such extension extent. The runlist then ends after the base record's extents, and with `errors=continue` every read past that point returns zeros. The data clusters are still allocated and intact.

## Evidence (Areia_10.nkx, inode 67301 = 0x106e5, 2,097,786,406 bytes)

- `system.ntfs_attrib` = 0x20 (ARCHIVE only): not compressed (0x800), sparse (0x200) or a reparse point (0x400). No `WofCompressedData` stream. WOF and LZNT1 are ruled out.
- Buffered reads, `mmap`, reads after `posix_fadvise(DONTNEED)` and `O_DIRECT` all return zeros from byte 132,513,792, which is page-aligned. The last non-zero byte is 132,513,791.
- The kernel log says `ntfs_attr_map_whole_runlist(): Failed to load full runlist: inode: 67301 highest_vcn: 0x7e5f last_vcn: 0x7d09b`. The boundary (0x7e5f+1) x 4096 = 132,513,792 is exactly the zero boundary, and (0x7d09b+1) x 4096 = 2,097,790,976 is the full allocation.
- The same log shows `ntfs_attr_value_is_valid(): Corrupt 0x80 attribute in MFT record 61194` and `ntfs_external_attr_find(): Base inode 0x106e5 contains corrupt attribute, mft 0xef0a, type 0x80`. So the `$DATA` extent in extension record 61194 is the one being rejected. The log holds 78 distinct inodes and 314 distinct extension records like this.
- FIEMAP maps 2 extents (256,768 + 2,048 clusters, about 132.5 MB). `st_blocks` still reports the full 2.0 GB allocation.
- Across the volume, no file with a multi-record `$DATA` reads correctly. The most fragmented healthy file (Solo_40.nkx) has 27 extents, which all fit in its base record. Every affected file's loaded runlist stops after 1 to 6 extents.
- Affected files were created by Windows (for example Areia_10: born 2025-09-25 13:40, last written 22:06, both before the new driver existed). Solo was extracted with 2021 timestamps and is unfragmented.
- ntfs3 (`fs/ntfs3/record.c`, `mi_enum_attr()`) validates the same fields more leniently. It only requires the extended header on the first segment (`!svcn && is_attr_ext`). It is expected to accept these records, but that is unverified until someone runs step 1.

The exact clause that fails can only be seen in the raw record bytes, and reading them needs root (see step 0).

## Scope (`python3 tools/ntfs_zero_scan.py`)

21,602 containers (489.7 GB) scanned. 200 files (387.8 GB) have an unmapped tail:

| Library | Unmapped GB |
|---|---:|
| Afflatus Chapter II Brass | 116.5 |
| Areia 1.2.0 | 81.8 |
| Audio Imperia CHORUS | 69.0 |
| Audio Imperia Dolce | 79.7 |
| Pacific Ensemble Strings | 30.1 |
| Una Corda Library | 10.7 |

These are exactly the libraries `RECOVERY.md` lists with zero-filled members. The two remaining zero-probe hits are fully mapped `.nkr` files with genuine zero padding. On import, the app now reports the count and the command below when the library sits on a kernel-`ntfs` mount (`src/import.rs`, `zero_read_warning`).

## Fix (run as root; the commands use fish syntax, the user's shell)

0. Optional, for an upstream report to linux-ntfs: dump the rejected record, which contains metadata only and no audio.

       sudo dd if='/mnt/MAIN_STORAGE/$MFT' bs=1024 skip=61194 count=1 status=none | xxd | head -40

1. Test ntfs3 without touching the live mount. This uses a read-only loop alias of the same partition.

       sudo modprobe ntfs3
       set LOOP (sudo losetup -r -f --show /dev/nvme0n1p1)
       sudo mkdir -p /mnt/ntfs3-test
       sudo mount -t ntfs3 -o ro,uid=1000,gid=1000 $LOOP /mnt/ntfs3-test
       python3 tools/ntfs_zero_scan.py /mnt/ntfs3-test/Libraries   # expect "unmapped tail: 0 files", exit 0
       sudo umount /mnt/ntfs3-test; sudo losetup -d $LOOP

   If ntfs3 refuses because the volume is dirty, add `force` to `-o` (safe for this read-only test).

2. Switch permanently. In `/etc/fstab`, change the type for `UUID=0D190E3C0D190E3C /mnt/MAIN_STORAGE` from `ntfs` to `ntfs3` and keep the options (`nofail,uid=1000,gid=1000,umask=0002,noatime,exec,x-systemd.device-timeout=10`). Close any programs using the drive, then run:

       sudo systemctl daemon-reload
       sudo umount /mnt/MAIN_STORAGE; and sudo mount /mnt/MAIN_STORAGE
       python3 tools/ntfs_zero_scan.py   # expect exit 0

   For a one-off remount without editing fstab:
   `sudo umount /mnt/MAIN_STORAGE; and sudo mount -t ntfs3 -o uid=1000,gid=1000,umask=0002,noatime /dev/nvme0n1p1 /mnt/MAIN_STORAGE`

If ntfs3 also shows unmapped tails, boot Windows and run `chkdsk D: /scan` (read-only) on that volume. The kernel log's own suggestion, "Unmount and run chkdsk", only applies if Windows agrees that the records are bad.

## Ruled out

- Userspace decoding: the rejected record holds the only map from file offsets to clusters for about 80% of each affected file. Without raw device access those clusters cannot be located, so no decoder can reach them.
- Password-free privilege paths: the user is not in `disk`, and `$MFT` is mode 000. The polkit actions `org.freedesktop.udisks2.open-device` and `filesystem-mount-system` both resolve to `auth_admin_keep` (wheel is the admin group, so a password is required). udisks also refuses a second mount of an already-mounted device.
