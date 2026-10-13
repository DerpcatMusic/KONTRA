| SerType | Kontakt class | native version | ni-file versions | status |
|---|---|---|---|---|
| 0x01 | class BAutomationObject | 0x71 |  | **no reader** |
| 0x02 | class BOutputConfiguration::OutputPartition | 0x70 |  | **no reader** |
| 0x05 | struct BLoop | 0x60 | undeclared | **native version missing** |
| 0x06 | class BParScript | 0x60 | 0x50, 0x60 | current |
| 0x07 | struct BParEnv | 0x90 |  | **no reader** |
| 0x08 | struct BParLFO | 0x73 |  | **no reader** |
| 0x09 | struct BParArp | 0x81 |  | **no reader** |
| 0x0A | struct BParEnvF | 0x60 |  | **no reader** |
| 0x0B | struct BParGlide | 0x51 |  | **no reader** |
| 0x0C | struct BParExternalMod | 0x105 | 0x100, 0x101, 0x102, 0x103, 0x104, 0x105 | current |
| 0x0D | struct BParInternalMod | 0x81 | 0x80, 0x81 | current |
| 0x0E | struct BParSrcMode | 0x106 |  | **no reader** |
| 0x0F | struct BParStartCriteria | 0x70 | 0x70 | current |
| 0x10 | struct BParFXDelay | 0x51 | 0x50, 0x51 | current |
| 0x11 | struct BParFXChorus | 0x51 |  | **no reader** |
| 0x12 | struct BParFXFlanger | 0x51 |  | **no reader** |
| 0x13 | struct BParFXGainer | 0x50 |  | **no reader** |
| 0x14 | struct BParFXPhaser | 0x51 |  | **no reader** |
| 0x15 | struct BParFXReverb | 0x50 |  | **no reader** |
| 0x16 | struct BParFXIRC | 0x70 |  | **no reader** |
| 0x17 | struct BParFXSendLevels | 0x51 | 0x50, 0x51 | current |
| 0x18 | struct BParFXFilter | 0x95 |  | **no reader** |
| 0x19 | struct BParFXCompressor | 0x70 |  | **no reader** |
| 0x1A | struct BParFXInverter | 0x60 |  | **no reader** |
| 0x1B | struct BParFXDYX | 0x60 |  | **no reader** |
| 0x1C | struct BParFXLimiter | 0x60 |  | **no reader** |
| 0x1D | struct BParFXShaper | 0x80 |  | **no reader** |
| 0x1D | struct BParFXSurroundPanner | 0x70 |  | **no reader** |
| 0x1E | struct BParFXDistortion | 0x60 |  | **no reader** |
| 0x1F | struct BParFXStereoSpread | 0x70 |  | **no reader** |
| 0x20 | struct BParFXLoFi | 0x70 |  | **no reader** |
| 0x21 | struct BParFXSkreamer | 0x50 |  | **no reader** |
| 0x22 | struct BParFXRotator | 0x50 | undeclared | **native version missing** |
| 0x23 | struct BParFXTwang | 0x51 |  | **no reader** |
| 0x24 | struct BParFXCabinet | 0x50 |  | **no reader** |
| 0x25 | struct BParFX | 0x50 | 0x50 | current |
| 0x26 | class BDyxMorphGroup | 0x50 |  | **no reader** |
| 0x27 | class BDyxMorphMap | 0x50 |  | **no reader** |
| 0x2A | class BSample | 0x10 |  | **no reader** |
| 0x2A | class BSamplePoolItem | 0x10 |  | **no reader** |
| 0x2B | class BVoiceLimit | 0x60 |  | **no reader** |
| 0x2D | class BZoneLevelEnv | 0x1 |  | **no reader** |
| 0x2E | class BZoneArraySer | 0x10 |  | **no reader** |
| 0x2F | class BGroupCompleteSer | 0x10 |  | **no reader** |
| 0x30 | class InsertFXChainPreset | 0x1 |  | **no reader** |
| 0x30 | class InsertFXPreset | 0x1 |  | **no reader** |
| 0x30 | class IntModPreset | 0x1 |  | **no reader** |
| 0x30 | class MultiScriptPreset | 0x1 |  | **no reader** |
| 0x30 | class ScriptPreset | 0x1 |  | **no reader** |
| 0x30 | class SendFXChainPreset | 0x1 |  | **no reader** |
| 0x30 | class SendFXPreset | 0x1 |  | **no reader** |
| 0x30 | class Snapshot | 0x1 |  | **no reader** |
| 0x30 | class SrcModePreset | 0x1 |  | **no reader** |
| 0x30 | class TablePreset | 0x1 |  | **no reader** |
| 0x3A | class BParameterArraySer<struct BParFX, 8> | 0x13 | 0x10, 0x11, 0x12, 0x13 | current |
| 0x3B | class BParameterArraySer<struct BParInternalMod, 16> | 0x13 | 0x10, 0x11, 0x12, 0x13 | current |
| 0x3C | class BParameterArraySer<struct BParExternalMod, 64> | 0x13 | 0x10, 0x11, 0x12, 0x13 | current |
| 0x3F | struct BParEnv_AHDSR | 0x11 | 0x10, 0x11 | current |
| 0x40 | struct BParEnv_FM7 | 0x12 | 0x11, 0x12 | current |
| 0x41 | struct BParEnv_DBD | 0x12 |  | **no reader** |
| 0x42 | struct BParFXTape | 0x10 |  | **no reader** |
| 0x43 | struct BParFXTrans | 0x10 |  | **no reader** |
| 0x44 | struct BParFXSSLGEQ | 0x11 |  | **no reader** |
| 0x46 | struct BParFXSSLGBusComp | 0x12 |  | **no reader** |
| 0x47 | class SaveSettings | 0x10 | 0x10 | current |
| 0x4A | class BParGroupDynamics | 0x10 |  | **no reader** |
| 0x4C | struct BParFXFBComp | 0x10 |  | **no reader** |
| 0x4D | struct BParFXJump | 0x10 |  | **no reader** |
| 0x4E | class QuickBrowseData | 0x1 | 0x1 | current |
| 0x4F | class BSnapshot | 0x3 |  | **no reader** |
| 0x50 | class BGroupSnapshot | 0x4 |  | **no reader** |
| 0x51 | class BSnapshotMetaData | 0x1 |  | **no reader** |
| 0x52 | struct BParFXVan51 | 0x10 |  | **no reader** |
| 0x53 | struct BParFXACBox | 0x10 |  | **no reader** |
| 0x54 | struct BParFXBassInvader | 0x52 |  | **no reader** |
| 0x54 | struct BParFXHotSolo | 0x10 |  | **no reader** |
| 0x55 | struct BParFXCat | 0x10 |  | **no reader** |
| 0x56 | struct BParFXDStortion | 0x10 |  | **no reader** |
| 0x57 | struct BParFXPlateReverb | 0x10 |  | **no reader** |
| 0x58 | struct BParFXCryWah | 0x10 |  | **no reader** |
| 0x59 | struct BParFXGaloisReverb | 0x10 |  | **no reader** |
| 0x5A | struct BParFXReplikaDelay | 0x12 |  | **no reader** |
| 0x5B | struct BParFXPhasis | 0x51 |  | **no reader** |
| 0x5C | struct BParFXFlair | 0x51 |  | **no reader** |
| 0x5D | struct BParFXChoral | 0x51 |  | **no reader** |
| 0x5E | struct BParFXCoreCell | 0x51 |  | **no reader** |
| 0x5F | struct BParFXHilbertLimiter | 0x51 |  | **no reader** |
| 0x60 | struct BParFXSupercharger | 0x51 |  | **no reader** |
| 0x61 | struct BParFXBassPro | 0x10 |  | **no reader** |
| 0x63 | struct BParFXPsycheDelay | 0x10 |  | **no reader** |
| 0x64 | struct BParFXRingModulator | 0x50 |  | **no reader** |
| 0x65 | struct BParFXRaum | 0x10 |  | **no reader** |
| 0x66 | struct BParFXBite | 0x10 |  | **no reader** |
| 0x67 | struct BParFXDirt | 0x10 |  | **no reader** |
| 0x68 | struct BParFXFreak | 0x10 |  | **no reader** |
| 0x69 | struct BParFXVibratoChorus | 0x10 |  | **no reader** |
| 0x6A | struct BParFXWowFlutter | 0x10 |  | **no reader** |
| 0x6B | struct BParFXTwinDelay | 0x10 |  | **no reader** |
| 0x6C | struct BParFXEPPreamps | 0x11 |  | **no reader** |
| 0x6D | class SER::BNISoundData | 0x1 |  | **no reader** |
| 0x6E | struct BParFXFuzz | 0x10 |  | **no reader** |
| 0x6F | struct BParFXBigFuzz | 0x10 |  | **no reader** |
| 0x70 | struct BParFXStereoTune | 0x10 |  | **no reader** |
| 0x71 | struct BParFXBeatMasher | 0x1 |  | **no reader** |
| 0x72 | struct BParFXGater | 0x1 |  | **no reader** |
| 0x73 | struct BParFXBeatSlicer | 0x1 |  | **no reader** |
| 0x74 | struct BParFXReverseGrain | 0x1 |  | **no reader** |
| 0x75 | struct BParFXTransposeStretch | 0x1 |  | **no reader** |
| 0x76 | struct BParFXSuperFast100 | 0x10 |  | **no reader** |
| 0x77 | struct BParFXChainsaw | 0x10 |  | **no reader** |
| 0x78 | struct BParFXSkreamerDlx | 0x10 |  | **no reader** |
| 0x79 | struct BParFXKolor | 0x10 |  | **no reader** |
| 0x7A | struct BParFXReverbDelight | 0x10 |  | **no reader** |
| 0xF00 | struct UIState | 0x60 |  | **no reader** |
| 0xF01 | struct CXBrowserUIState | 0x10 |  | **no reader** |
| 0xF02 | struct SessionState | 0x10 |  | **no reader** |
| 0x2710 | class BApp::BSplashDialog | 0x2710 |  | **no reader** |
| 0x2710 | class NI::NGL::StandardSplashDialog2 | 0x2710 |  | **no reader** |

118 native classes: 14 current, 2 behind Kontakt 8.13.1, 102 with no reader.
