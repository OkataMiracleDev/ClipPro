# Source this file (bash) before cargo commands: source scripts/env.sh
#
# Uses the portable toolchain in third_party/ when it is there (scripts/bootstrap.ps1): MSVC and
# the Windows SDK, LLVM, CMake, Ninja. Without it (GitHub's Windows runners, or a machine with
# Visual Studio installed) the installed Visual Studio, LLVM and CMake are used; run this from a
# developer shell there, or after ilammy/msvc-dev-cmd in CI. FFmpeg always comes from
# third_party/ffmpeg (bootstrap.ps1 -Only ffmpeg).
_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
_tp="$_root/third_party"
_w() { cygpath -w "$1"; }
_msvc_ver="$(ls "$_tp/msvc/VC/Tools/MSVC" 2>/dev/null | sort | tail -1 || true)"
if [ -n "$_msvc_ver" ]; then
  _sdk_ver="$(ls "$_tp/msvc/Windows Kits/10/bin" 2>/dev/null | grep '^10\.' | sort | tail -1 || true)"
  _vc="$_tp/msvc/VC/Tools/MSVC/$_msvc_ver"
  _sdk="$_tp/msvc/Windows Kits/10"
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
  export PATH="$_vc/bin/Hostx64/x64:$_sdk/bin/$_sdk_ver/x64:$PATH"
  # CMake builds (whisper.cpp) use Ninja with the portable MSVC: there is no Visual Studio
  # installation for CMake's default generator to find.
  export CMAKE_GENERATOR=Ninja
  export CMAKE_C_COMPILER="$(_w "$_vc/bin/Hostx64/x64/cl.exe")"
  export CMAKE_CXX_COMPILER="$(_w "$_vc/bin/Hostx64/x64/cl.exe")"
  unset _sdk_ver _vc _sdk
elif [ -z "${CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER:-}" ]; then
  # Git Bash puts its own coreutils link.exe ahead of MSVC's on PATH; point cargo at the
  # linker next to the developer shell's cl.exe instead.
  _cl="$(command -v cl.exe 2>/dev/null || true)"
  if [ -n "$_cl" ]; then
    export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER="$(_w "$(dirname "$_cl")/link.exe")"
  fi
  unset _cl
fi
if [ -d "$_tp/llvm/bin" ]; then
  export LIBCLANG_PATH="$(_w "$_tp/llvm/bin")"
elif [ -z "${LIBCLANG_PATH:-}" ] && [ -f "/c/Program Files/LLVM/bin/libclang.dll" ]; then
  export LIBCLANG_PATH='C:\Program Files\LLVM\bin'
fi
export FFMPEG_DIR="$(_w "$_tp/ffmpeg")"
export PATH="$HOME/.cargo/bin:$_tp/ffmpeg/bin:$_tp/llvm/bin:$_tp/cmake/bin:$_tp/tools/bin:$PATH"
# AI models for tests and dev runs: CLIPPRO_MODELS from this shell, else the user's Windows
# environment (shells started before it was set don't see it), else target/models.
if [ -z "${CLIPPRO_MODELS:-}" ]; then
  _m="$(reg query 'HKCU\Environment' //v CLIPPRO_MODELS 2>/dev/null | sed -n 's/.*REG_[A-Z_]*SZ[[:space:]]*//p' | tr -d '\r' || true)"
  export CLIPPRO_MODELS="${_m:-$(_w "$_root/target/models")}"
  unset _m
fi
unset _root _tp _msvc_ver
