import { loader } from "fumadocs-core/source";
import { docs } from "../../.source";

/** The /docs pages (F-WB-02). */
export const source = loader({ baseUrl: "/docs", source: docs.toFumadocsSource() });
