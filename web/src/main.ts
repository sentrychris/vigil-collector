import { createApp } from "vue";
import { createPinia } from "pinia";
import piniaPluginPersistedstate from "pinia-plugin-persistedstate";

import { http, useHttp } from "./plugins/http";
import { useWebsocket } from "./plugins/websocket";

import App from "./App.vue";
import router from "./router";

import { FontAwesomeIcon } from "@fortawesome/vue-fontawesome";
import "./utilities/icons";

import "bootstrap";
import "./assets/main.scss";

const pinia = createPinia()
  .use(piniaPluginPersistedstate)
  .use(({ store }) => {
    store.http = http;
  });

createApp(App)
  .use(pinia)
  .use(router)
  .use(useHttp)
  .use(useWebsocket)
  .component("font-awesome-icon", FontAwesomeIcon)
  .mount("#app");
