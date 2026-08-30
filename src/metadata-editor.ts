export type TechnicalMetadata = {
  frameRate: string;
  resolution: string;
  videoCodec: string;
  audioCodec: string;
  colorSpace: string;
  colorPrimaries: string;
  transferCharacteristics: string;
  rotationOrientation: string;
  timecode: string;
  c2pa: string;
  encoderVersion: string;
};

export type EditableMetadata = {
  title: string;
  artistAuthor: string;
  creator: string;
  comment: string;
  description: string;
  copyright: string;
  creationTime: string;
  modificationTime: string;
  encoder: string;
  encodedBy: string;
  software: string;
  version: string;
  publisher: string;
  genre: string;
  language: string;
  location: string;
  keywords: string;
  projectName: string;
  projectId: string;
  assetId: string;
  uuid: string;
  source: string;
  editSoftware: string;
  exportPreset: string;
  encoderVersion: string;
  handlerName: string;
  gps: string;
  cameraDevice: string;
  customMetadata: string;
  xmp: string;
};

export type VideoMetadata = {
  technical: TechnicalMetadata;
  editable: EditableMetadata;
};

type TechnicalKey = keyof TechnicalMetadata;
type EditableKey = keyof EditableMetadata;

type EditableField = {
  key: EditableKey;
  label: string;
  placeholder?: string;
  multiline?: boolean;
};

const technicalFields: Array<[TechnicalKey, string]> = [
  ["frameRate", "Frame Rate：フレームレート"],
  ["resolution", "Resolution：解像度"],
  ["videoCodec", "Video Codec：映像コーデック"],
  ["audioCodec", "Audio Codec：音声コーデック"],
  ["colorSpace", "Color Space：色空間"],
  ["colorPrimaries", "Color Primaries：色域情報"],
  ["transferCharacteristics", "Transfer Characteristics：HDR・SDR"],
  ["rotationOrientation", "Rotation / Orientation：動画の向き"],
  ["timecode", "Timecode：編集用タイムコード"],
  ["c2pa", "C2PA：制作元・編集履歴・AI生成情報"],
];

const editableFields: EditableField[] = [
  { key: "title", label: "Title：動画タイトル" },
  { key: "artistAuthor", label: "Artist / Author：制作者名" },
  { key: "creator", label: "Creator：作成者" },
  { key: "comment", label: "Comment：コメント", multiline: true },
  { key: "description", label: "Description：動画の説明", multiline: true },
  { key: "copyright", label: "Copyright：著作権表記" },
  { key: "creationTime", label: "Creation Time：作成日時", placeholder: "例: 2026-08-30T12:00:00+09:00" },
  { key: "modificationTime", label: "Modification Time：更新日時", placeholder: "例: 2026-08-30T12:00:00+09:00" },
  { key: "encoder", label: "Encoder：使用したエンコーダー名" },
  { key: "encodedBy", label: "Encoded By：エンコードしたソフト名・制作者" },
  { key: "software", label: "Software：使用ソフト名" },
  { key: "version", label: "Version：自作ソフトのバージョン" },
  { key: "publisher", label: "Publisher：公開者・発行者" },
  { key: "genre", label: "Genre：ジャンル" },
  { key: "language", label: "Language：言語", placeholder: "例: ja" },
  { key: "location", label: "Location：撮影・制作場所" },
  { key: "keywords", label: "Keywords：検索用キーワード", multiline: true },
  { key: "projectName", label: "Project Name：プロジェクト名" },
  { key: "projectId", label: "Project ID：プロジェクト識別番号" },
  { key: "assetId", label: "Asset ID：動画固有ID" },
  { key: "uuid", label: "UUID：一意の識別子" },
  { key: "source", label: "Source：元データ・入力元" },
  { key: "editSoftware", label: "Edit Software：編集ソフト名" },
  { key: "exportPreset", label: "Export Preset：書き出し設定名" },
  { key: "encoderVersion", label: "Encoder Version：FFmpeg・libx264などのバージョン" },
  { key: "handlerName", label: "Handler Name：映像ストリームの処理名" },
  { key: "gps", label: "GPS情報：緯度・経度", placeholder: "例: +35.6812+139.7671/" },
  { key: "cameraDevice", label: "Camera / Device：撮影機器名" },
  { key: "customMetadata", label: "Custom Metadata：独自メタデータ", placeholder: "1行につき key=value", multiline: true },
  { key: "xmp", label: "XMP：拡張メタデータ", placeholder: "XMP文字列またはXML", multiline: true },
];

function emptyEditableMetadata(): EditableMetadata {
  return Object.fromEntries(editableFields.map(({ key }) => [key, ""])) as EditableMetadata;
}

function fileTitle(path: string) {
  return (path.split(/[\\/]/).pop() || "video").replace(/\.[^.]+$/, "");
}

export class MetadataEditor {
  private readonly inputs = new Map<EditableKey, HTMLInputElement | HTMLTextAreaElement>();

  constructor(
    private readonly technicalContainer: HTMLElement,
    private readonly editableContainer: HTMLElement,
  ) {
    this.createTechnicalFields();
    this.createEditableFields();
  }

  populate(metadata: VideoMetadata, outputPath: string, appVersion: string) {
    this.technicalContainer.querySelectorAll<HTMLElement>("[data-metadata-technical]").forEach((element) => {
      const key = element.dataset.metadataTechnical as TechnicalKey;
      element.textContent = metadata.technical[key] || "―";
    });
    const values: EditableMetadata = { ...emptyEditableMetadata(), ...metadata.editable };
    if (!values.title) values.title = fileTitle(outputPath);
    if (!values.encoder) values.encoder = "FFmpeg";
    if (!values.encodedBy) values.encodedBy = "QuickVideoMaker";
    if (!values.software) values.software = "QuickVideoMaker";
    if (!values.version) values.version = appVersion;
    if (!values.source) values.source = outputPath;
    if (!values.editSoftware) values.editSoftware = "QuickVideoMaker";
    if (!values.exportPreset) values.exportPreset = "H.264 / AAC MP4";
    if (!values.encoderVersion) values.encoderVersion = metadata.technical.encoderVersion;
    for (const [key, input] of this.inputs) input.value = values[key] || "";
  }

  collect(): EditableMetadata {
    const values = emptyEditableMetadata();
    for (const [key, input] of this.inputs) values[key] = input.value;
    return values;
  }

  setDisabled(disabled: boolean) {
    for (const input of this.inputs.values()) input.disabled = disabled;
  }

  private createTechnicalFields() {
    const fragment = document.createDocumentFragment();
    for (const [key, label] of technicalFields) {
      const row = document.createElement("div");
      row.className = "metadata-readonly-row";
      const term = document.createElement("dt");
      term.textContent = label;
      const value = document.createElement("dd");
      value.dataset.metadataTechnical = key;
      value.textContent = "―";
      row.append(term, value);
      fragment.append(row);
    }
    this.technicalContainer.replaceChildren(fragment);
  }

  private createEditableFields() {
    const fragment = document.createDocumentFragment();
    for (const field of editableFields) {
      const label = document.createElement("label");
      label.className = `metadata-edit-field${field.multiline ? " metadata-edit-field-wide" : ""}`;
      const text = document.createElement("span");
      text.textContent = field.label;
      const input = field.multiline ? document.createElement("textarea") : document.createElement("input");
      input.dataset.metadataKey = field.key;
      input.placeholder = field.placeholder || "空欄の場合は保存しません";
      if (input instanceof HTMLTextAreaElement) input.rows = field.key === "xmp" ? 5 : 3;
      label.append(text, input);
      this.inputs.set(field.key, input);
      fragment.append(label);
    }
    this.editableContainer.replaceChildren(fragment);
  }
}