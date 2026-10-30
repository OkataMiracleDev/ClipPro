# Source this file (bash) to use the portable toolchain in third_party/.
# Usage: source scripts/env.sh
_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
_tp="$_root/third_party"
_msvc_ver="$(ls "$_tp/msvc/VC/Tools/MSVC" 2>/dev/null | sort | tail -1)"
_sdk_ver="$(ls "$_tp/msvc/Windows Kits/10/bin" 2>/dev/null | grep '^10\.' | sort | tail -1)"
_vc="$_tp/msvc/VC/Tools/MSVC/$_msvc_ver"
_sdk="$_tp/msvc/Windows Kits/10"
_w() { cygpath -w "$1"; }
export VCINSTALLDIR="$(_w "$_tp/msvc/VC")\\"
export VCToolsInstallDir="$(_w "$_vc")\\"
export VSCMD_ARG_HOST_ARCH=x64
export VSCMD_ARG_TGT_ARCH=x64
export VCToolsVersion="$_msvc_ver"
export WindowsSDKVersion="$_sdk_ver\\"
export WindowsSdkDir="$(_w "$_sdk")\\"
export INCLUDE="$(_w "$_vc/include");$(_w "$_sdk/Include/$_sdk_ver/ucrt");$(_w "$_sdk/Include/$_sdk_ver/shared");$(_w "$_sdk/Include/$_sdk_ver/um");$(_w "$_sdk/Include/$_sdk_ver/winrt");$(_w "$_sdk/Include/$_sdk_ver/cppwinrt")"
export LIB="$(_w "$_vc/lib/x64");$(_w "$_sdk/Lib/$_sdk_ver/ucrt/x64");$(_w "$_sdk/Lib/$_sdk_ver/um/x64")"
export CC_x86_64_pc_windows_msvc="$(_w "$_vc/bin/Hostx64/x64/cl.exe")"
export CXX_x86_64_pc_windows_msvc="$(_w "$_vc/bin/Hostx64/x64/cl.exe")"
export AR_x86_64_pc_windows_msvc="$(_w "$_vc/bin/Hostx64/x64/lib.exe")"
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="$(_w "$_vc/bin/Hostx64/x64/link.exe")"
export RC="$(_w "$_sdk/bin/$_sdk_ver/x64/rc.exe")"
export FXC="$(_w "$_sdk/bin/$_sdk_ver/x64/fxc.exe")"
export FFMPEG_DIR="$(_w "$_tp/ffmpeg")"
export LIBCLANG_PATH="$(_w "$_tp/llvm/bin")"
export PATH="$HOME/.cargo/bin:$_vc/bin/Hostx64/x64:$_sdk/bin/$_sdk_ver/x64:$_tp/ffmpeg/bin:$_tp/llvm/bin:$_tp/cmake/bin:$_tp/tools/bin:$PATH"
# CMake builds (whisper.cpp for P12.3) use Ninja with the portable MSVC above.
export CMAKE_GENERATOR=Ninja
export CMAKE_C_COMPILER="$(_w "$_vc/bin/Hostx64/x64/cl.exe")"
export CMAKE_CXX_COMPILER="$(_w "$_vc/bin/Hostx64/x64/cl.exe")"
# AI models for tests and dev runs (the app uses %LOCALAPPDATA%\ClipPro\models).
export CLIPPRO_MODELS="${CLIPPRO_MODELS:-$(_w "$_root/target/models")}"
unset _root _tp _msvc_ver _sdk_ver _vc _sdk
