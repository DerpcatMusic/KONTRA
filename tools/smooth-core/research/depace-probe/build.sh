#!/bin/sh
# Build the two Windows VST3 clients used for the depacing work.
#
# Needs mingw-w64 and the VST3 SDK that ships with JUCE. Point VST3_SDK at it:
#
#   VST3_SDK=/path/to/juce/modules/juce_audio_processors/format_types/VST3_SDK \
#       ./build.sh
#
# Run them under Wine against a copy of the patched image, e.g.
#
#   wine create.exe soothe3-native.vst3 direct
#
# See ../depacing.md for what each mode proves.

set -e

VST3_SDK="${VST3_SDK:-/mnt/Projects/JUCE/modules/juce_audio_processors/format_types/VST3_SDK}"
CXX="${CXX:-x86_64-w64-mingw32-g++}"

if [ ! -d "$VST3_SDK" ]; then
    echo "VST3_SDK not found: $VST3_SDK" >&2
    exit 1
fi

COMMON="-std=c++17 -O1 -I$VST3_SDK -static -lole32"
SDK_SOURCES="$VST3_SDK/pluginterfaces/base/funknown.cpp \
             $VST3_SDK/pluginterfaces/base/coreiids.cpp \
             $VST3_SDK/public.sdk/source/vst/vstinitiids.cpp"

# shellcheck disable=SC2086
$CXX $COMMON -o inventory.exe inventory.cpp $SDK_SOURCES
# shellcheck disable=SC2086
$CXX $COMMON -o create.exe create.cpp $SDK_SOURCES

echo "built inventory.exe and create.exe"
