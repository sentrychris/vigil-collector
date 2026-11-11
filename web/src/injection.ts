// Injection keys for exposing the http and websocket singletons to components
// via Vue's provide/inject. Currently unused at call sites (consumers import
// the singletons directly), but kept so components can opt into DI with
// `inject(injectHttp)` / `inject(injectWebsocket)`.
import type { InjectionKey } from "vue";
import type { HttpMaker } from "./plugins/http";
import type { WebsocketMaker } from "./plugins/websocket";

export const injectHttp: InjectionKey<HttpMaker> = Symbol("http");
export const injectWebsocket: InjectionKey<WebsocketMaker> = Symbol("websocket");
