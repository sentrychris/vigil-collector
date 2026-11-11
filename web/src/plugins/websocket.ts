import type { App } from "vue";
import { config } from "@/config";
import { injectWebsocket } from "@/injection";
import { WebsocketStatus } from "@/enums/StatusCodes";

export class WebsocketMaker {
  private instance: WebSocket | null = null;

  private data: any;

  private get websocket(): WebSocket {
    return this.instance != null ? this.instance : this.init();
  }

  init() {
    const websocket = new WebSocket(config.api.urls.websocket);

    websocket.onopen = () => {
      console.log("websocket is connected!");
    };

    websocket.onmessage = (value) => {
      this.data = value;
    };

    this.instance = websocket;

    return websocket;
  }

  private handleError(error: Response) {
    const { status } = error;

    switch (status) {
      case WebsocketStatus.InternalServerError: {
        break;
      }
      case WebsocketStatus.ClosedProtocolError: {
        // Handle closed with protocol error
        break;
      }
      case WebsocketStatus.ClosedNoStatus: {
        // Handle closed with no status
        break;
      }
    }

    return Promise.reject(error);
  }
}

const websocket = new WebsocketMaker();

export const useWebsocket = {
  install(app: App) {
    app.provide(injectWebsocket, websocket);
  },
};
