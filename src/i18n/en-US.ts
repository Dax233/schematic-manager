/** 界面文案（英语）。
 *
 * 用 `typeof zhCN` 声明：中文那边加了键而这里没加会直接编译报错，缺翻译不会
 * 拖到运行期才发现。 */
import zhCN from "./zh-CN";

const enUS: typeof zhCN = {
  lang: {
    label: "Language",
    zhCN: "简体中文",
    enUS: "English",
  },
  titleBar: {
    appName: "Schematic Manager",
  },
  toolbar: {
    addDir: "Add directory",
    loadedDirs: "Loaded directories ({count})",
    noDirs: "No directories loaded yet",
    fileCount: "{count} files",
    remove: "Remove",
    removeTitle: "Remove this directory from the list (files on disk are kept)",
    searchPlaceholder: "Search name/author/file name…",
    replace: "Block replacement",
    convert: "Version conversion",
    duplicates: "Duplicate scan",
    backups: "Backups and logs",
    setup: "Version library",
    update: "Updates",
    rmc: "RMC Schematics",
    rmcTitle: "RMC Schematics (entry reserved; available once the add-on is wired up)",
    totalFiles: "{total} schematics",
    matchedFiles: ", {matched} matching",
  },
  app: {
    splitterAria: "Resize the directory sidebar",
    splitterTitle: "Drag to resize, double-click to reset",
    selectedFile: "Selected: {name}",
  },
  setup: {
    title: "Version library",
    hint: "Scans that directory (subdirectories included) for any resource file it can import. You can also download over the network from the official endpoint or the BMCL mirror.",
    entityMissing:
      "The entity texture library has not been built yet, so entity previews will miss textures — preparing any single version again fills it in.",
    dirPlaceholder: "Directory path (subdirectories included)",
    browse: "Browse…",
    scan: "Scan and import",
    sourceOfficial: "Official endpoint",
    sourceBmcl: "BMCL mirror",
    startSetup: "Prepare checked versions ({count})",
    sourceHint:
      "The official endpoint downloads from Mojang and is often unreachable from mainland China; switch to the BMCL mirror when it fails. Versions found locally are imported directly and cost no download traffic; the checkboxes are for versions missing from the manifest that need downloading.",
    scanWarnings: "{count} entries could not be read during the scan and were skipped",
    loadingStatus: "Reading version library state…",
    checkAll: "Select all",
    uncheckAll: "Clear all",
    version: "Version",
    dataVersion: "Data version",
    state: "State",
    result: "Progress / result",
    ready: "Ready",
    local: "Installed locally",
    missing: "Download needed",
    progressUnknown: "Downloading…",
    progressBuilding: "Building…",
  },
  displayName: {
    missing: "Not present in the language table",
  },
};

export default enUS;
