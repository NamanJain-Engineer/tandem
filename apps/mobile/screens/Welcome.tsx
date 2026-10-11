// Not paired yet: scan a code, paste a link, or try the demo. Also shows
// pairing in progress, a failed pairing and an unpaired phone.
import { useState } from "react";
import { ScrollView, StyleSheet, View } from "react-native";
import type { Phase } from "@tandem/remote/session";
import { t } from "../lib/strings";
import { Body, Button, Card, Field, Loading, Notice, Small, Title } from "../ui/kit";
import { space } from "../ui/tokens";

export function Welcome({
  phase,
  onScan,
  onPaste,
  onDemo,
}: {
  phase: Phase;
  onScan: () => void;
  onPaste: (url: string) => void;
  onDemo: () => void;
}) {
  const [link, setLink] = useState("");
  if (phase.kind === "pairing" || phase.kind === "loading") {
    return <Loading label={phase.kind === "pairing" ? t("pair.pairing") : t("loading")} />;
  }
  return (
    <ScrollView contentContainerStyle={st.wrap} keyboardShouldPersistTaps="handled">
      <Title>{t("app.name")}</Title>
      <Body muted>{t("app.tagline")}</Body>
      {phase.kind === "pair_failed" && (
        <Card>
          <Body>{t("pair.failed")}</Body>
          <Notice tone="error">{phase.message}</Notice>
        </Card>
      )}
      {phase.kind === "revoked" && <Notice tone="warning">{t("pair.revoked")}</Notice>}
      <Card>
        <Body>{t("pair.title")}</Body>
        <Small>{t("pair.body")}</Small>
        <Small>{t("pair.lanHint")}</Small>
        <Button label={t("pair.scan")} variant="primary" onPress={onScan} testID="pair-scan" />
      </Card>
      <Card>
        <Small>{t("pair.paste")}</Small>
        <Field
          label={t("pair.paste")}
          placeholder={t("pair.pastePlaceholder")}
          value={link}
          onChangeText={setLink}
          autoCapitalize="none"
          autoCorrect={false}
          inputMode="url"
        />
        <Button label={t("pair.pasteGo")} disabled={!link.includes("#p=") && !link.includes("?p=")} onPress={() => onPaste(link.trim())} />
      </Card>
      <View style={st.demo}>
        <Button label={t("pair.demo")} onPress={onDemo} testID="pair-demo" />
        <Small>{t("pair.demoNote")}</Small>
      </View>
    </ScrollView>
  );
}

const st = StyleSheet.create({
  wrap: { padding: space[5], gap: space[4] },
  demo: { gap: space[2] },
});
