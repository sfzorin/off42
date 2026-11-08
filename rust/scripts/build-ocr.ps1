# Pinned sources; static libraries and static CRT, including all transitive codecs.
$ErrorActionPreference = 'Stop'
$baseline = '6fdd23fa0a9479a9f9a1c48178696f60cf39255d'
$source = (Resolve-Path "$PSScriptRoot/../native/ocr").Path
$root = Join-Path $source '../../build/ocr-sdk'
New-Item -ItemType Directory -Force $root | Out-Null
$root = (Resolve-Path $root).Path
$vcpkg = Join-Path $root 'vcpkg'
if (-not (Test-Path "$vcpkg/.git")) {
  git clone --filter=blob:none https://github.com/microsoft/vcpkg.git $vcpkg
  if ($LASTEXITCODE -ne 0) { throw 'vcpkg clone failed' }
}
git -C $vcpkg checkout --detach $baseline
if ($LASTEXITCODE -ne 0) { throw 'vcpkg pin checkout failed' }
& "$vcpkg/bootstrap-vcpkg.bat" -disableMetrics
if ($LASTEXITCODE -ne 0) { throw 'vcpkg bootstrap failed' }
$build = Join-Path $root 'cmake'
$installed = Join-Path $root 'installed'
cmake -S $source -B $build -G Ninja "-DCMAKE_TOOLCHAIN_FILE=$vcpkg/scripts/buildsystems/vcpkg.cmake" '-DVCPKG_TARGET_TRIPLET=x64-windows-static' "-DVCPKG_INSTALLED_DIR=$installed" '-DCMAKE_BUILD_TYPE=Release' "-DCMAKE_INSTALL_PREFIX=$root"
if ($LASTEXITCODE -ne 0) { throw 'OCR SDK configure failed' }
cmake --build $build --config Release
if ($LASTEXITCODE -ne 0) { throw 'OCR SDK build failed' }
cmake --install $build --config Release
if ($LASTEXITCODE -ne 0) { throw 'OCR SDK install failed' }
Copy-Item "$installed/x64-windows-static/lib/*.lib" "$root/lib/"
New-Item -ItemType Directory -Force "$root/licenses" | Out-Null
Get-ChildItem "$installed/x64-windows-static/share" -Directory | ForEach-Object {
  if (Test-Path "$($_.FullName)/copyright") { Copy-Item "$($_.FullName)/copyright" "$root/licenses/$($_.Name)-copyright" }
}
if (-not (Test-Path "$root/licenses/tesseract-copyright")) { throw 'OCR license missing' }
$env:OFF42_OCR_SDK = $root
if ($env:GITHUB_ENV) { "OFF42_OCR_SDK=$root" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append }
