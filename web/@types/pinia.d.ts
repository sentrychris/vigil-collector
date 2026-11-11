import "pinia";
import type { HttpMaker } from "../src/plugins/http";

declare module "pinia" {
  export interface PiniaCustomProperties {
    http: HttpMaker;
  }

  export interface DefineStoreOptionsBase<S, Store> {
    persist?: boolean | object;
  }
}
