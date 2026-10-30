// Vibezeo Remote's components, drawn only with ui/tokens (the desktop's
// packages/ui tokens). Every screen uses these.
import type { ReactNode } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
  type StyleProp,
  type TextInputProps,
  type ViewStyle,
} from "react-native";
import type { AgentStatus } from "@tandem/protocol";
import { color, font, radius, rn, space } from "./tokens";

export function Title({ children }: { children: ReactNode }) {
  return <Text style={s.title}>{children}</Text>;
}

export function Body({ children, muted, style }: { children: ReactNode; muted?: boolean; style?: StyleProp<ViewStyle> }) {
  return <Text style={[s.body, muted && s.muted, style as object]}>{children}</Text>;
}

export function Small({ children }: { children: ReactNode }) {
  return <Text style={s.small}>{children}</Text>;
}

type ButtonProps = {
  label: string;
  onPress: () => void;
  /** When the tap begins (Allow judges a tap by it, F-TL-12 AC 21). */
  onPressIn?: () => void;
  /** Why it's off, for screen readers. */
  accessibilityHint?: string;
  variant?: "primary" | "secondary" | "danger";
  size?: "sm" | "md";
  disabled?: boolean;
  testID?: string;
};

export function Button({
  label,
  onPress,
  onPressIn,
  accessibilityHint,
  variant = "secondary",
  size = "md",
  disabled,
  testID,
}: ButtonProps) {
  const bg = variant === "primary" ? color.accent : variant === "danger" ? color.danger : color.surface4;
  const fg = variant === "primary" ? color.accentFg : variant === "danger" ? color.dangerFg : color.text;
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled: !!disabled }}
      accessibilityHint={accessibilityHint}
      testID={testID}
      onPress={onPress}
      onPressIn={onPressIn}
      disabled={disabled}
      style={({ pressed }) => [
        s.button,
        size === "sm" && s.buttonSm,
        { backgroundColor: bg, opacity: disabled ? 0.45 : pressed ? 0.8 : 1 },
      ]}
    >
      <Text style={[s.buttonText, size === "sm" && s.buttonTextSm, { color: fg }]}>{label}</Text>
    </Pressable>
  );
}

export function Card({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  return <View style={[s.card, style]}>{children}</View>;
}

export function Row({ children, style }: { children: ReactNode; style?: StyleProp<ViewStyle> }) {
  return <View style={[s.row, style]}>{children}</View>;
}

export function Badge({ label, tone = "idle" }: { label: string; tone?: keyof typeof STATUS | "accent" }) {
  const c = tone === "accent" ? color.accent : STATUS[tone];
  return (
    <View style={[s.badge, { borderColor: c }]}>
      <Text style={[s.badgeText, { color: c }]}>{label}</Text>
    </View>
  );
}

const STATUS: Record<AgentStatus, string> = {
  working: color.working,
  waiting: color.waiting,
  done: color.done,
  idle: color.idle,
  failed: color.error,
};

export function StatusDot({ status }: { status: AgentStatus }) {
  return <View accessibilityElementsHidden style={[s.dot, { backgroundColor: STATUS[status] }]} />;
}

export function Field(props: TextInputProps & { label: string }) {
  const { label, style, ...rest } = props;
  return (
    <TextInput
      accessibilityLabel={label}
      placeholderTextColor={color.textSubtle}
      style={[s.field, style]}
      {...rest}
    />
  );
}

export function Notice({ children, tone = "info" }: { children: ReactNode; tone?: "info" | "error" | "warning" }) {
  const c = tone === "error" ? color.danger : tone === "warning" ? color.waiting : color.textMuted;
  return <Text style={[s.notice, { color: c }]} accessibilityRole={tone === "error" ? "alert" : "text"}>{children}</Text>;
}

export function Loading({ label }: { label: string }) {
  return (
    <View style={s.center} accessibilityLabel={label}>
      <ActivityIndicator color={color.accent} />
      <Text style={[s.small, { marginTop: space[2] }]}>{label}</Text>
    </View>
  );
}

export function Empty({ label }: { label: string }) {
  return (
    <View style={s.center}>
      <Text style={s.muted}>{label}</Text>
    </View>
  );
}

export function Divider() {
  return <View style={s.divider} />;
}

export const s = StyleSheet.create({
  title: { color: color.text, fontSize: font.xl, fontWeight: "600" },
  body: { color: color.text, fontSize: font.base, lineHeight: 22 },
  muted: { color: color.textMuted, fontSize: font.base },
  small: { color: color.textSubtle, fontSize: font.sm },
  button: {
    minHeight: 44,
    paddingHorizontal: space[4],
    borderRadius: radius.sm,
    alignItems: "center",
    justifyContent: "center",
  },
  buttonSm: { minHeight: 36, paddingHorizontal: space[3] },
  buttonText: { fontSize: font.base, fontWeight: "600" },
  buttonTextSm: { fontSize: font.sm },
  card: {
    backgroundColor: color.surface2,
    borderColor: rn(color.border),
    borderWidth: 1,
    borderRadius: radius.md,
    padding: space[4],
    gap: space[2],
  },
  row: { flexDirection: "row", alignItems: "center", gap: space[2] },
  badge: { borderWidth: 1, borderRadius: radius.xs, paddingHorizontal: 6, paddingVertical: 1 },
  badgeText: { fontSize: 12, fontWeight: "600" },
  dot: { width: 10, height: 10, borderRadius: 5 },
  field: {
    minHeight: 44,
    color: color.text,
    backgroundColor: color.surface4,
    borderColor: rn(color.borderStrong),
    borderWidth: 1,
    borderRadius: radius.sm,
    paddingHorizontal: space[3],
    fontSize: font.base,
  },
  notice: { fontSize: font.sm, lineHeight: 19 },
  center: { flex: 1, alignItems: "center", justifyContent: "center", padding: space[6] },
  divider: { height: 1, backgroundColor: rn(color.border) },
});
