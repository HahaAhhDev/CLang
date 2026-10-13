# CLang VSCode extension

Local sources for CLang coloring in VSCode. No store publish yet.

## Install

Copy this folder to your extensions dir and restart VSCode:

```
mkdir -p ~/.vscode/extensions
cp -r editors/vscode ~/.vscode/extensions/clang-0.6.0
```

Then open any `.clng` file. The grammar lives in
`syntaxes/clang.tmGrammar.json`, pairs in
`language-configuration.json`, starter snippets in
`snippets/clang.json`. Logo source is
`../../assets/clang-logo.svg`, rasterized to
`images/icon.png` at 128 by 128.

## Publish later

Needs node plus vsce plus a marketplace token:

```
npm install -g @vscode/vsce
vsce package
vsce publish
```
