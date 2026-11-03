import "@elyra/ui/tokens.css";
import { mount } from "svelte";
import App from "./App.svelte";
import Quick from "./Quick.svelte";

// The tray's popover (RFC 0019) opens at /quick.
const page = location.pathname.startsWith("/quick") ? Quick : App;
export default mount(page, { target: document.getElementById("app") });
