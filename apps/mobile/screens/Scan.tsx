// Scans the pairing code with the camera (expo-camera's built-in scanner).
import { useRef } from "react";
import { StyleSheet, View } from "react-native";
import { CameraView, useCameraPermissions } from "expo-camera";
import { t } from "../lib/strings";
import { Body, Button, Loading, Notice } from "../ui/kit";
import { color, space } from "../ui/tokens";

export function Scan({ onCode, onCancel }: { onCode: (data: string) => void; onCancel: () => void }) {
  const [permission, request] = useCameraPermissions();
  const done = useRef(false);
  if (!permission) return <Loading label={t("loading")} />;
  if (!permission.granted) {
    return (
      <View style={st.center}>
        <Body>{t("scan.permission")}</Body>
        {permission.canAskAgain ? (
          <Button label={t("scan.allow")} variant="primary" onPress={() => void request()} />
        ) : (
          <Notice tone="warning">{t("scan.denied")}</Notice>
        )}
        <Button label={t("scan.cancel")} onPress={onCancel} />
      </View>
    );
  }
  return (
    <View style={st.fill}>
      <CameraView
        style={st.fill}
        facing="back"
        barcodeScannerSettings={{ barcodeTypes: ["qr"] }}
        onBarcodeScanned={(r) => {
          if (done.current || !r.data.includes("p=")) return;
          done.current = true;
          onCode(r.data);
        }}
      />
      <View style={st.bar}>
        <Body>{t("scan.title")}</Body>
        <Button label={t("scan.cancel")} onPress={onCancel} />
      </View>
    </View>
  );
}

const st = StyleSheet.create({
  fill: { flex: 1 },
  center: { flex: 1, justifyContent: "center", padding: space[6], gap: space[4] },
  bar: { padding: space[4], gap: space[3], backgroundColor: color.surface1 },
});
