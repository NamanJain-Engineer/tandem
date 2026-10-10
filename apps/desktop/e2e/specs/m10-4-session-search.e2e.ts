import { freshOffline } from "../support/app";
import { lead, makeTeammate, newChat, sendAndWait, type Teammate } from "../support/agents";

/** The hits `tandem__session_search` gave, from the fake engine's reply. */
async function search(
  chat: string,
  query: string,
): Promise<Array<{ chatId: string; title: string; snippet: string }>> {
  const replies = await sendAndWait(chat, `mcp tandem__session_search {"query":"${query}"}`);
  const said = replies.join(" ");
  const json = said.replace(/^.*?The tool said: /, "");
  return JSON.parse(json) as Array<{ chatId: string; title: string; snippet: string }>;
}

// E2E-M10-4 Session search (F-AG-11): only user and assistant prose is
// indexed, and a teammate only finds its own chats.
describe("E2E-M10-4 session search", () => {
  let a: Teammate;
  let b: Teammate;
  let aChat = "";
  let bChat = "";

  before(async function () {
    this.timeout(90_000);
    await freshOffline();
    a = await makeTeammate("Avery");
    // Free: one teammate besides the Lead.
    b = await lead();
    aChat = await newChat(a.id);
    bChat = await newChat(b.id);
    // "okapi" only in a tool's output; "pelican" in the assistant's reply.
    await sendAndWait(aChat, "/tool-output ipako");
    await sendAndWait(aChat, "pelican");
    await sendAndWait(bChat, "flamingo");
  });

  it("F-AG-11.1 a word only in tool output finds nothing", async function () {
    this.timeout(30_000);
    expect(await search(aChat, "okapi")).toEqual([]);
  });

  it("F-AG-11.1 a word in an assistant message finds that chat", async function () {
    this.timeout(30_000);
    const hits = await search(aChat, "pelican");
    expect(hits.map((h) => h.chatId)).toEqual([aChat]);
    expect(hits[0]?.snippet).toContain("[pelican]");
  });

  it("F-AG-11.2 teammate A's search returns none of B's chats", async function () {
    this.timeout(30_000);
    expect(await search(aChat, "flamingo")).toEqual([]);
    const own = await search(bChat, "flamingo");
    expect(own.map((h) => h.chatId)).toEqual([bChat]);
  });
});
