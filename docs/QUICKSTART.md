# Quickstart

## 1. Bootstrap (PowerShell 7+)

```powershell
./scripts/bootstrap.ps1
```

## 2. Run Tests + Verification

```powershell
./scripts/verify-loop.ps1 -Iterations 5
./scripts/run-tests.ps1
./scripts/coverage.ps1
```

## 3. Build

```powershell
./scripts/build-debug.ps1
./scripts/build-release.ps1
```

## 4. Validate Extensions

```powershell
cargo run -p copperd -- verify --extensions-dir ./extensions
```

## 5. Generate main.ts from descriptor

```powershell
cargo run -p copperd -- generate-main ./extensions/sort-downloads/descriptor.json
```
