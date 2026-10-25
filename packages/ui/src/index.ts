export {
  APPROVAL_LABELS,
  ApprovalCard,
  type ApprovalCardKind,
  type ApprovalCardLabels,
  type ApprovalCardProps,
} from "./components/ApprovalCard";
export {
  AVATAR_HUES,
  Avatar,
  AvatarStack,
  avatarHue,
  fnv1a32,
  hueIndex,
  initials,
  type AvatarProps,
} from "./components/Avatar";
export { Badge, Chip, type BadgeVariant, type ChipProps } from "./components/Badge";
export { Button, buttonClass, type ButtonProps, type ButtonSize, type ButtonVariant } from "./components/Button";
export {
  Checkbox,
  RadioGroup,
  Switch,
  type CheckboxProps,
  type RadioGroupProps,
  type RadioItem,
  type SwitchProps,
} from "./components/Choice";
export {
  CommandPalette,
  type CommandPaletteProps,
  type PaletteGroup,
  type PaletteItem,
} from "./components/CommandPalette";
export { QuickPicker, type QuickPickerItem, type QuickPickerProps } from "./components/QuickPicker";
export {
  Banner,
  Callout,
  EmptyState,
  KeyValue,
  ListRow,
  PageHeader,
  SettingsGroup,
  SettingsRow,
  Stepper,
  type ListRowProps,
  type SettingsRowProps,
  type Tone,
} from "./components/Composites";
export {
  ConfirmDialog,
  Dialog,
  Sheet,
  type ConfirmDialogProps,
  type DialogProps,
  type SheetProps,
} from "./components/Dialog";
export {
  DiffView,
  LARGE_DIFF_LINES,
  diffLineKey,
  parsePatch,
  type DiffComments,
  type DiffFile,
  type DiffLine,
  type DiffLineTarget,
  type DiffViewLabels,
} from "./components/DiffView";
export {
  COMPOSER_LABELS,
  Composer,
  MicButton,
  insertText,
  mentionAt,
  type ComposerAttachment,
  type ComposerDictation,
  type ComposerHandle,
  type ComposerLabels,
  type ComposerProps,
  type ComposerSelect,
  type MentionItem,
} from "./components/Composer";
export {
  Input,
  SecretInput,
  Textarea,
  type InputProps,
  type SecretInputProps,
  type TextareaProps,
} from "./components/Field";
export { EngineBadge, EngineGlyph } from "./components/EngineGlyph";
export { Heatmap, heatLevel, type HeatmapDay } from "./components/Heatmap";
export { CodeEditor, languageFor, type CodeEditorProps } from "./components/CodeEditor";
export { NoteEditor, type NoteEditorHandle, type NoteEditorProps } from "./components/NoteEditor";
export { QRCode, type QRCodeProps } from "./components/QRCode";
export { HotkeyRecorder, type HotkeyRecorderProps, type RecordedKeys } from "./components/HotkeyRecorder";
export { NOTE_FORMATS, continueList, taskBoxes, tickAt, toggleBold, toggleBullet, toggleItalic, toggleTask, toggleUnderline, toggleWrap, type NoteFormat } from "./components/noteCommands";
export {
  FILE_ROW_HEIGHT,
  FILE_TREE_LABELS,
  FileTree,
  GIT_LETTER,
  VIRTUALIZE_ABOVE,
  visibleRows,
  type FileTreeChange,
  type FileTreeLabels,
  type FileTreeNode,
  type FileTreeProps,
} from "./components/FileTree";
export { IconButton, type IconButtonProps } from "./components/IconButton";
export { Kbd, type KbdProps } from "./components/Kbd";
export { ContextMenu, Menu, orderEntries, type MenuEntry, type MenuProps } from "./components/Menu";
export {
  MARKDOWN_MAX,
  parseInline,
  parseMarkdown,
  safeHref,
  splitStable,
  type Block as MarkdownBlock,
  type Inline as MarkdownInline,
} from "./components/markdown";
export { CodeBlock, LARGE_PREVIEW, MarkdownView, type CodeBlockProps, type MarkdownViewProps } from "./components/MarkdownView";
export { Popover, type PopoverProps } from "./components/Popover";
export {
  ContextMeter,
  ProgressBar,
  UsageMeter,
  meterTone,
  tokenCount,
  type ContextMeterProps,
  type ProgressBarProps,
  type UsageMeterProps,
} from "./components/Progress";
export { SegmentedControl, type Segment, type SegmentedControlProps } from "./components/SegmentedControl";
export {
  Combobox,
  Select,
  type ComboboxProps,
  type SelectOption,
  type SelectProps,
} from "./components/Select";
export { Skeleton, SkeletonGate, useDelayedFlag } from "./components/Skeleton";
export { Spinner, type SpinnerProps } from "./components/Spinner";
export { STATUS_LABEL, StatusDot, type Status } from "./components/StatusDot";
export { Tabs, type TabItem, type TabsProps } from "./components/Tabs";
export {
  ToolCallBlock,
  ToolGroup,
  ToolOutput,
  type ToolCallBlockProps,
  type ToolCallStatus,
  type ToolGroupProps,
  type ToolIcon,
} from "./components/ToolCallBlock";
export { ToastProvider, useToast, type ToastInput, type ToastTone } from "./components/Toast";
export {
  ANNOUNCE_INTERVAL_MS,
  LiveAnnouncer,
  announce,
  configureAnnouncer,
  resetAnnouncer,
} from "./components/LiveAnnouncer";
export { Tooltip, TooltipProvider, type TooltipProps } from "./components/Tooltip";
export { WaveformRing, ringStats, type WaveformRingProps, type WaveformState } from "./components/WaveformRing";
export { cx } from "./cx";
export { isMac, shortcutCaps, shortcutLabel } from "./platform";
export {
  ThemeProvider,
  UiProvider,
  useTheme,
  type Density,
  type MotionSetting,
  type TextSize,
  type ThemeProviderProps,
  type ThemeSetting,
  type ThemeState,
} from "./theme/ThemeProvider";
export {
  tandemDay,
  tandemHighContrast,
  tandemNight,
  terminalThemeFor,
  type TerminalTheme,
  type TerminalThemeSetting,
} from "./theme/terminalThemes";
