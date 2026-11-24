# Input mode artwork

The user supplied the Illustrator artwork for these icons on 2026-10-12.
The original files remain unchanged outside the repository:

- `a_template.ai` (2026-10-13 07:28 版) -> `mode_kana.svg`
  SHA-256: `8aa83858acea82809963c1c8a03546cb778d53c1b4aa930403e0472df2b557ae`
- `A ラスタライズ前.ai` -> `mode_latin.svg`
  SHA-256: `43b848f84a40953329a78fdcd9c39d11ad408b15dbfa873bd4a7fcc896b5e2e9`

The PDF-compatible artwork was extracted with `pdftocairo -svg`. Only the
glyph paths were retained, with their original coordinates and transforms.
Reference guides and the kana working layer's 55% opacity were removed.
Both Latin subpaths share one fill operation to avoid antialiasing seams.
The 1000-by-1000 artboard is preserved. Color is supplied by the host theme.

Both the popup indicator and taskbar icons use these SVG sources. Rebuild the
four ICO files using Windows Node.js and installed Microsoft Edge:

```powershell
node tools/build-mode-icons.cjs C:\path\to\isolated-evidence
```

This uses a temporary headless browser profile, not the user's browser profile.
The generator checks all ten sizes (16 through 256 pixels) for nonempty,
monochrome pixels, matching light/dark alpha masks, and distinct mode artwork.
PNG previews and a contact sheet are written into the evidence directory.
Each ICO embeds all ten transparent PNG frames. Rebuild both client DLL
architectures after regenerating resources; rebuild the UI after SVG edits.
The application logo `azookey.ico` is intentionally unchanged.
