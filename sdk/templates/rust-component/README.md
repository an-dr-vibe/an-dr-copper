# Copper Rust Component template

Create a project copy with:

```powershell
./scripts/new-wasm-extension.ps1 -Id my-extension -Name "My Extension"
```

Build, validate, and package it with:

```powershell
./scripts/build-wasm-extension.ps1 -ExtensionDir ./extensions/my-extension -Package
```

The component implements Bones' generated `extension` world through
`copper-component-sdk`. Native work is requested with the versioned
`copper.bus/1` envelope and completes asynchronously through `copper-jobs`.
