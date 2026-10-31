// Paired: agents, approvals, threads and teammate chats, routines and the
// phone's settings, each with empty, loading, error and populated states.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  Alert,
  FlatList,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Switch,
  Text,
  useWindowDimensions,
  View,
} from "react-native";
import type {
  PhoneAgent,
  PhoneApproval,
  PhoneReview,
  PhoneThread,
  PhoneTranscriptItem,
} from "@tandem/protocol";
import type { RemoteSession, SessionView } from "@tandem/remote/session";
import type { Prefs } from "../lib/prefs";
import {
  NO_ANSWERS,
  SETTLE_MS,
  afterReply,
  afterSnapshot,
  allowTakesTap,
  approvalActions,
  now,
  pressedAt,
  takesAnswer,
} from "../lib/approvals";
import { checkText, lineKind, reviewLines, ruleText, statText } from "../lib/review";
import { t, type StringKey } from "../lib/strings";
import {
  Badge,
  Body,
  Button,
  Card,
  Empty,
  Field,
  Loading,
  Notice,
  Row,
  Small,
  StatusDot,
  Title,
  s as kit,
} from "../ui/kit";
import { color, font, rn, space } from "../ui/tokens";

export type Tab = "agents" | "approvals" | "threads" | "routines" | "review" | "settings";
const TABS: Tab[] = ["agents", "approvals", "threads", "routines", "review", "settings"];

type Props = {
  session: RemoteSession;
  view: SessionView;
  demo: boolean;
  prefs: Prefs;
  pushAvailable: boolean;
  version: string;
  onPrefs: (p: Prefs) => void;
  onLeaveDemo: () => void;
  onDemoNext: () => void;
};

export function Home(props: Props) {
  const { view, demo } = props;
  const [tab, setTab] = useState<Tab>("agents");
  const [thread, setThread] = useState<PhoneThread | null>(null);
  const [typing, setTyping] = useState<PhoneAgent | null>(null);
  const waiting = view.snapshot?.approvals.filter((a) => !a.queued).length ?? 0;
  // Tablets (and phones in landscape wide enough) show the list and the open
  // thread or agent side by side.
  const wide = useWindowDimensions().width >= 700;

  const detail = thread ? (
    <ThreadView
      session={props.session}
      thread={thread}
      canSend={!!view.grants?.chat}
      onBack={() => setThread(null)}
    />
  ) : typing ? (
    <TypeView session={props.session} agent={typing} onBack={() => setTyping(null)} />
  ) : null;

  const list =
    view.snapshot === null ? (
      view.snapshotError ? (
        <View style={kit.center}>
          <Notice tone="error">{view.snapshotError}</Notice>
          <Button label={t("error.retry")} onPress={() => void props.session.refresh()} />
        </View>
      ) : tab === "settings" ? (
        <SettingsTab {...props} />
      ) : (
        <Loading
          label={t(view.connection === "online" ? "loading" : (`conn.${view.connection}` as StringKey))}
        />
      )
    ) : tab === "agents" ? (
      <AgentsTab view={view} onType={setTyping} />
    ) : tab === "approvals" ? (
      <ApprovalsTab session={props.session} view={view} />
    ) : tab === "threads" ? (
      <ThreadsTab view={view} onOpen={setThread} />
    ) : tab === "routines" ? (
      <RoutinesTab session={props.session} view={view} />
    ) : tab === "review" ? (
      <ReviewTab session={props.session} view={view} />
    ) : (
      <SettingsTab {...props} />
    );

  return (
    <View style={st.fill}>
      <View style={st.header}>
        <View style={{ flex: 1 }}>
          <Text style={st.desktop} numberOfLines={1}>
            {view.desktop || t("app.name")}
          </Text>
          <Row>
            <View
              style={[st.pill, { backgroundColor: view.connection === "online" ? color.done : color.idle }]}
            />
            <Small>{t(`conn.${view.connection}` as StringKey)}</Small>
          </Row>
        </View>
      </View>
      {demo && (
        <View style={st.demo}>
          <Small>{t("demo.banner")}</Small>
          <Row>
            <Button size="sm" label={t("demo.next")} onPress={props.onDemoNext} />
            <Button size="sm" label={t("demo.leave")} onPress={props.onLeaveDemo} />
          </Row>
        </View>
      )}
      {wide ? (
        <View style={st.split}>
          <View style={st.listPane}>{list}</View>
          <View style={st.fill}>{detail ?? <Empty label={t("split.empty")} />}</View>
        </View>
      ) : (
        <View style={st.fill}>{detail ?? list}</View>
      )}
      <View style={st.tabs} accessibilityRole="tablist">
        {TABS.map((k) => (
          <Pressable
            key={k}
            accessibilityRole="tab"
            accessibilityState={{ selected: tab === k }}
            testID={`tab-${k}`}
            style={st.tab}
            onPress={() => {
              setTab(k);
              setThread(null);
              setTyping(null);
            }}
          >
            <Text style={[st.tabText, tab === k && st.tabOn]}>{t(`tab.${k}` as StringKey)}</Text>
            {k === "approvals" && waiting > 0 && <Text style={st.count}>{waiting}</Text>}
          </Pressable>
        ))}
      </View>
    </View>
  );
}

function AgentsTab({ view, onType }: { view: SessionView; onType: (a: PhoneAgent) => void }) {
  const agents = view.snapshot?.agents ?? [];
  if (agents.length === 0) return <Empty label={t("agents.empty")} />;
  return (
    <FlatList
      contentContainerStyle={st.list}
      data={agents}
      keyExtractor={(a) => a.key}
      renderItem={({ item: a }) => (
        <Card>
          <Row>
            <StatusDot status={a.status} />
            <Text style={st.itemTitle} numberOfLines={1}>
              {a.title}
            </Text>
            <Badge label={t(`status.${a.status}` as StringKey)} tone={a.status} />
          </Row>
          <Small>{[a.engine, a.workspace].filter(Boolean).join(" · ")}</Small>
          {a.preview ? <Body muted>{a.preview}</Body> : null}
          {a.paneId &&
            (view.grants?.terminal ? (
              <Button size="sm" label={t("agents.type")} onPress={() => onType(a)} />
            ) : (
              <Small>{t("agents.typeNeeds")}</Small>
            ))}
        </Card>
      )}
    />
  );
}

function ApprovalsTab({ session, view }: { session: RemoteSession; view: SessionView }) {
  const snapshot = view.snapshot;
  const approvals = snapshot?.approvals ?? [];
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Each answer is sent once: a second tap while one is on its way, or
  // after it went and before the list drops the card or shows it queued,
  // sends nothing. Refs, so taps handled in one batch see each other (the
  // review of ce0d5815).
  const sending = useRef(new Set<string>());
  const answers = useRef(NO_ANSWERS);
  // A queued Allow the computer dropped gives its card back, before the
  // card's buttons can take a tap.
  useLayoutEffect(() => {
    if (snapshot) answers.current = afterSnapshot(answers.current, snapshot.approvals);
  }, [snapshot]);
  async function decide(a: PhoneApproval, d: "allow" | "deny") {
    if (sending.current.has(a.id) || !takesAnswer(answers.current, a.id)) return;
    sending.current.add(a.id);
    setBusy(a.id);
    try {
      const r = await session.decide(a, d);
      answers.current = afterReply(answers.current, a.id, r);
      setError(r.ok ? null : r.message);
    } finally {
      sending.current.delete(a.id);
      setBusy(null);
    }
  }
  // A scroll moves every card under the finger: each Allow waits again. The
  // ref is what a tap is judged by, set as the scroll lands; the state only
  // runs the wait's timer.
  const scrolled = useRef(0);
  const [scrolledAt, setScrolledAt] = useState(0);
  return (
    <ScrollView
      contentContainerStyle={st.list}
      scrollEventThrottle={100}
      onScroll={() => {
        scrolled.current = now();
        setScrolledAt(scrolled.current);
      }}
    >
      <Small>{t("approvals.armedNote")}</Small>
      {!view.grants?.approvals && <Notice tone="warning">{t("approvals.needsGrant")}</Notice>}
      {error && <Notice tone="error">{error}</Notice>}
      {approvals.length === 0 && <Empty label={t("approvals.empty")} />}
      {approvals.map((a) => (
        <ApprovalCard
          key={a.id}
          a={a}
          granted={!!view.grants?.approvals}
          busy={busy === a.id}
          scrolled={scrolled}
          scrolledAt={scrolledAt}
          decide={decide}
        />
      ))}
    </ScrollView>
  );
}

/** One approval. The list refreshes on its own and cards above come and go,
 * so Allow waits until this card has held still (its request, its place and
 * size, and the list's scroll) for SETTLE_MS, and a tap begun before that
 * allows nothing (F-TL-12 AC 21). Deny never waits.
 *
 * A tap is judged by refs set as each change lands and by the touch's own
 * time: read from the last render, a layout handled in the same batch as a
 * quick tap wasn't seen yet, and a tap the busy thread handled late looked
 * late enough (the review of ce0d5815, SF-2). One monotonic clock
 * throughout. */
function ApprovalCard({
  a,
  granted,
  busy,
  scrolled,
  scrolledAt,
  decide,
}: {
  a: PhoneApproval;
  granted: boolean;
  busy: boolean;
  scrolled: { current: number };
  scrolledAt: number;
  decide: (a: PhoneApproval, d: "allow" | "deny") => Promise<void>;
}) {
  const can = approvalActions(a, granted);
  const request = `${a.id}\n${a.summary}\n${a.detail}`;
  // When its request or its place last changed.
  const changed = useRef(now());
  const place = useRef<string | null>(null);
  const [changedAt, setChangedAt] = useState(() => changed.current);
  useLayoutEffect(() => {
    changed.current = now();
    setChangedAt(changed.current);
  }, [request]);
  const since = Math.max(changedAt, scrolledAt);
  const [steady, setSteady] = useState(false);
  useEffect(() => {
    setSteady(false);
    const timer = setTimeout(() => setSteady(true), Math.max(0, since + SETTLE_MS - now()));
    return () => clearTimeout(timer);
  }, [since]);
  const pressed = useRef<number | null>(null);
  return (
    <View
      onLayout={(e) => {
        const { x, y, width, height } = e.nativeEvent.layout;
        const at = `${x},${y},${width},${height}`;
        if (at === place.current) return;
        place.current = at;
        changed.current = now();
        setChangedAt(changed.current);
      }}
    >
      <Card>
        <Body>{a.summary}</Body>
        <Small>{a.source}</Small>
        {a.detail && a.detail !== a.summary ? <Text style={st.mono}>{a.detail}</Text> : null}
        {a.queued ? (
          <Notice tone="warning">{t("approvals.queued")}</Notice>
        ) : (
          <Row>
            {can.deskOnly ? (
              <Notice tone="warning">{t("approvals.deskOnly")}</Notice>
            ) : (
              <Button
                size="sm"
                variant="primary"
                label={t("approvals.allow")}
                disabled={busy || !can.allow || !steady}
                accessibilityHint={steady ? undefined : t("approvals.allowWaiting")}
                onPressIn={(stamp) => {
                  pressed.current = pressedAt(now(), stamp);
                }}
                onPress={() => {
                  // Each tap its own: a screen reader's activation has no
                  // press-in, and an old refused touch mustn't judge it.
                  const began = pressed.current;
                  pressed.current = null;
                  if (allowTakesTap(Math.max(changed.current, scrolled.current), began, now())) {
                    void decide(a, "allow");
                  }
                }}
              />
            )}
            <Button
              size="sm"
              variant="danger"
              label={t("approvals.deny")}
              disabled={busy || !can.deny}
              onPress={() => void decide(a, "deny")}
            />
          </Row>
        )}
      </Card>
    </View>
  );
}

function ThreadsTab({ view, onOpen }: { view: SessionView; onOpen: (t: PhoneThread) => void }) {
  const threads = view.snapshot?.threads ?? [];
  if (threads.length === 0) return <Empty label={t("threads.empty")} />;
  return (
    <FlatList
      contentContainerStyle={st.list}
      data={threads}
      keyExtractor={(x) => x.id}
      renderItem={({ item }) => (
        <Pressable accessibilityRole="button" onPress={() => onOpen(item)}>
          <Card>
            <Row>
              <Text style={st.itemTitle} numberOfLines={1}>
                {item.title}
              </Text>
              {item.teammate && <Badge label={t("threads.teammate")} tone="accent" />}
              <Small>{t(`status.${item.state === "interrupted" ? "idle" : item.state}` as StringKey)}</Small>
            </Row>
          </Card>
        </Pressable>
      )}
    />
  );
}

function ThreadView({
  session,
  thread,
  canSend,
  onBack,
}: {
  session: RemoteSession;
  thread: PhoneThread;
  canSend: boolean;
  onBack: () => void;
}) {
  const [items, setItems] = useState<PhoneTranscriptItem[] | null>(null);
  const [before, setBefore] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const load = useCallback(async () => {
    const r = await session.readThread(thread.id);
    if (r.ok) {
      setItems(r.data.items);
      setBefore(r.data.before);
      setError(null);
    } else setError(r.message);
  }, [session, thread.id]);
  useEffect(() => {
    void load();
  }, [load]);
  async function older() {
    if (before === null) return;
    const r = await session.readThread(thread.id, before);
    if (r.ok) {
      setItems((cur) => [...r.data.items, ...(cur ?? [])]);
      setBefore(r.data.before);
    }
  }
  async function send() {
    setSending(true);
    const r = await session.sendToThread(thread.id, text);
    setSending(false);
    if (r.ok) {
      setText("");
      void load();
    } else setError(r.message);
  }
  return (
    <KeyboardAvoidingView style={st.fill} behavior={Platform.OS === "ios" ? "padding" : undefined}>
      <Row style={st.subhead}>
        <Button size="sm" label={t("thread.back")} onPress={onBack} />
        <Text style={st.itemTitle} numberOfLines={1}>
          {thread.title}
        </Text>
      </Row>
      {items === null && !error && <Loading label={t("loading")} />}
      {error && <Notice tone="error">{error}</Notice>}
      {items !== null && (
        <ScrollView contentContainerStyle={st.list}>
          {before !== null && <Button size="sm" label={t("thread.older")} onPress={() => void older()} />}
          {items.length === 0 && <Empty label={t("thread.empty")} />}
          {items.map((m, i) => (
            <View
              key={`${m.at}-${i}`}
              style={[st.msg, m.role === "user" ? st.msgUser : m.role === "tool" ? st.msgTool : null]}
            >
              <Text style={m.role === "tool" ? st.mono : kit.body}>{m.text}</Text>
            </View>
          ))}
        </ScrollView>
      )}
      <View style={st.composer}>
        {canSend ? (
          <Row>
            <View style={st.fill}>
              <Field
                label={t("thread.placeholder")}
                placeholder={t("thread.placeholder")}
                value={text}
                onChangeText={setText}
                multiline
              />
            </View>
            <Button
              label={t("thread.send")}
              variant="primary"
              disabled={!text.trim() || sending}
              onPress={() => void send()}
            />
          </Row>
        ) : (
          <Small>{t("thread.readOnly")}</Small>
        )}
      </View>
    </KeyboardAvoidingView>
  );
}

function TypeView({
  session,
  agent,
  onBack,
}: {
  session: RemoteSession;
  agent: PhoneAgent;
  onBack: () => void;
}) {
  const [text, setText] = useState("");
  const [state, setState] = useState<{ ok: boolean; message: string } | null>(null);
  async function send() {
    if (!agent.paneId) return;
    const r = await session.typeToPane(agent.paneId, text);
    setState(r.ok ? { ok: true, message: t("type.sent") } : { ok: false, message: r.message });
    if (r.ok) setText("");
  }
  return (
    <ScrollView contentContainerStyle={st.list} keyboardShouldPersistTaps="handled">
      <Row>
        <Button size="sm" label={t("thread.back")} onPress={onBack} />
      </Row>
      <Title>{t("type.title", { agent: agent.title })}</Title>
      <Small>{t("type.body")}</Small>
      <Field
        label={t("type.placeholder")}
        placeholder={t("type.placeholder")}
        value={text}
        onChangeText={setText}
        multiline
      />
      <Button label={t("type.send")} variant="primary" disabled={!text.trim()} onPress={() => void send()} />
      {state && <Notice tone={state.ok ? "info" : "error"}>{state.message}</Notice>}
    </ScrollView>
  );
}

function RoutinesTab({ session, view }: { session: RemoteSession; view: SessionView }) {
  const routines = view.snapshot?.routines ?? [];
  const [error, setError] = useState<string | null>(null);
  return (
    <ScrollView contentContainerStyle={st.list}>
      {!view.grants?.routines && <Notice tone="warning">{t("routines.needsGrant")}</Notice>}
      {error && <Notice tone="error">{error}</Notice>}
      {routines.length === 0 && <Empty label={t("routines.empty")} />}
      {routines.map((r) => (
        <Card key={r.id}>
          <Row>
            <Text style={st.itemTitle} numberOfLines={1}>
              {r.name}
            </Text>
            {r.running && <Badge label={t("routines.running")} tone="working" />}
            {r.paused && <Badge label={t("routines.paused")} tone="idle" />}
          </Row>
          <Button
            size="sm"
            label={t("routines.run")}
            disabled={!view.grants?.routines || r.paused || r.running}
            onPress={() => void session.runRoutine(r.id).then((res) => setError(res.ok ? null : res.message))}
          />
        </Card>
      ))}
    </ScrollView>
  );
}

const LINE_COLOR = { meta: color.textMuted, add: color.done, del: color.error, context: color.text } as const;

/** F-CN-09: the review item Conductor is on (else the first to review) and
 * its diff, read only. Approving and merging stay on the computer. */
function ReviewTab({ session, view }: { session: RemoteSession; view: SessionView }) {
  const [review, setReview] = useState<PhoneReview | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    void session.reviewCurrent().then((r) => {
      if (!live) return;
      if (r.ok) {
        setReview(r.data);
        setError(null);
      } else setError(r.message);
    });
    return () => {
      live = false;
    };
    // Asked again only when the queue changed on the computer.
  }, [session, view.snapshot?.reviewRev]);
  if (error) return <Notice tone="error">{error}</Notice>;
  if (!review) return <Loading label={t("loading")} />;
  const item = review.item;
  if (!item) return <Empty label={t("review.empty")} />;
  const check = checkText(item);
  const lines = reviewLines(review);
  return (
    <ScrollView contentContainerStyle={st.list}>
      <Card>
        <Row>
          <Text style={st.itemTitle} numberOfLines={1}>
            {item.title}
          </Text>
          {review.position > 0 && (
            <Small>{t("review.position", { n: review.position, total: review.total })}</Small>
          )}
        </Row>
        <Small>{[item.engine, item.workspace, item.branch].filter(Boolean).join(" · ")}</Small>
        {item.changed && <Badge label={t("review.changed")} tone="waiting" />}
        {item.closing ? <Body>{item.closing}</Body> : null}
        <Small>{statText(item)}</Small>
        {item.risks.map((r) => (
          <Badge key={r.rule} label={ruleText(r.rule)} tone="accent" />
        ))}
        {check && <Small>{check}</Small>}
        <Small>{t("review.onComputer")}</Small>
      </Card>
      {review.files.map((f) =>
        f.withheld ? null : (
          <Card key={f.path}>
            <Text style={st.mono}>
              {f.path} +{f.additions} −{f.deletions}
            </Text>
            <ScrollView horizontal>
              <View>
                {f.patch.split("\n").map((l, i) => (
                  <Text key={i} style={[st.mono, { color: LINE_COLOR[lineKind(l)] }]}>
                    {l || " "}
                  </Text>
                ))}
              </View>
            </ScrollView>
            {f.truncated && <Small>{t("review.truncated")}</Small>}
          </Card>
        ),
      )}
      {!lines.empty && lines.withheld.map((w) => <Notice key={w}>{w}</Notice>)}
      {!lines.empty && lines.omitted && <Small>{lines.omitted}</Small>}
    </ScrollView>
  );
}

function SettingsTab(props: Props) {
  const { view, prefs, onPrefs } = props;
  const grants = view.grants;
  function confirmUnpair() {
    Alert.alert(t("settings.unpair"), t("settings.unpairConfirm", { desktop: view.desktop }), [
      { text: t("settings.cancel"), style: "cancel" },
      { text: t("settings.unpair"), style: "destructive", onPress: () => void props.session.unpair() },
    ]);
  }
  return (
    <ScrollView contentContainerStyle={st.list}>
      <Card>
        <Small>{t("settings.computer")}</Small>
        <Body>{view.desktop}</Body>
        <Small>{`${t("settings.connection")}: ${t(`conn.${view.connection}` as StringKey)}`}</Small>
      </Card>
      <Card>
        <Small>{t("settings.grants")}</Small>
        {(["chat", "terminal", "approvals", "routines"] as const).map((g) => (
          <Row key={g} style={st.between}>
            <Body>{t(`grant.${g}` as StringKey)}</Body>
            <Badge label={t(grants?.[g] ? "grant.on" : "grant.off")} tone={grants?.[g] ? "done" : "idle"} />
          </Row>
        ))}
        <Small>{t("settings.grantsNote")}</Small>
      </Card>
      <Card>
        <Row style={st.between}>
          <Body>{t("settings.notifications")}</Body>
          <Switch
            accessibilityLabel={t("settings.notifications")}
            value={prefs.notifications}
            onValueChange={(v) => onPrefs({ ...prefs, notifications: v })}
            trackColor={{ true: color.accent, false: color.surface4 }}
          />
        </Row>
        <Small>{t("settings.notificationsNote")}</Small>
      </Card>
      <Card>
        <Row style={st.between}>
          <Body>{t("settings.push")}</Body>
          <Switch
            accessibilityLabel={t("settings.push")}
            value={prefs.push && props.pushAvailable}
            disabled={!props.pushAvailable || props.demo}
            onValueChange={(v) => onPrefs({ ...prefs, push: v })}
            trackColor={{ true: color.accent, false: color.surface4 }}
          />
        </Row>
        <Small>{props.pushAvailable ? t("settings.pushNote") : t("settings.pushUnavailable")}</Small>
      </Card>
      {!props.demo && (
        <Button label={t("settings.unpair")} variant="danger" onPress={confirmUnpair} testID="unpair" />
      )}
      <Small>{t("settings.version", { version: props.version })}</Small>
    </ScrollView>
  );
}

const st = StyleSheet.create({
  fill: { flex: 1 },
  split: { flex: 1, flexDirection: "row" },
  listPane: { width: 380, borderRightWidth: 1, borderRightColor: rn(color.border) },
  header: {
    flexDirection: "row",
    alignItems: "center",
    paddingHorizontal: space[4],
    paddingVertical: space[3],
    borderBottomWidth: 1,
    borderBottomColor: rn(color.border),
    backgroundColor: color.surface1,
  },
  desktop: { color: color.text, fontSize: font.md, fontWeight: "600" },
  pill: { width: 8, height: 8, borderRadius: 4 },
  demo: { padding: space[3], gap: space[2], backgroundColor: rn(color.accentSoft) },
  list: { padding: space[4], gap: space[3] },
  itemTitle: { flex: 1, color: color.text, fontSize: font.base, fontWeight: "600" },
  mono: {
    color: color.textMuted,
    fontFamily: Platform.OS === "ios" ? "Menlo" : "monospace",
    fontSize: font.sm,
  },
  tabs: {
    flexDirection: "row",
    borderTopWidth: 1,
    borderTopColor: rn(color.border),
    backgroundColor: color.surface1,
  },
  tab: { flex: 1, alignItems: "center", paddingVertical: space[3], minHeight: 48 },
  tabText: { color: color.textMuted, fontSize: 12, fontWeight: "600" },
  tabOn: { color: color.accent },
  count: {
    position: "absolute",
    top: 2,
    right: 2,
    color: color.accentFg,
    backgroundColor: color.waiting,
    borderRadius: 8,
    paddingHorizontal: 5,
    fontSize: 11,
    overflow: "hidden",
  },
  subhead: { padding: space[3], borderBottomWidth: 1, borderBottomColor: rn(color.border) },
  msg: { padding: space[3], borderRadius: 8, backgroundColor: color.surface2, gap: space[1] },
  msgUser: { backgroundColor: color.surface4, alignSelf: "flex-end", maxWidth: "90%" },
  msgTool: { backgroundColor: "transparent" },
  composer: {
    padding: space[3],
    borderTopWidth: 1,
    borderTopColor: rn(color.border),
    backgroundColor: color.surface1,
  },
  between: { justifyContent: "space-between" },
});
