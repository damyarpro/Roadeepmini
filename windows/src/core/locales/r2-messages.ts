// Registers the strings of the island's waiting cards, live diff and coding
// agents' wording (r2-en.ts, r2-fa.ts). Imported for its effect by every view
// module that uses them, so each one works on its own (tests included).

import { registerMessages } from "../i18n";
import { r2En } from "./r2-en";
import { r2Fa } from "./r2-fa";

registerMessages(r2En, r2Fa);
