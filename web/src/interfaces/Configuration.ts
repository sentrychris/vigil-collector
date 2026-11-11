import type { ConnectionTypes } from "./types/ConnectionTypes";

interface BaseConfiguration {
  name: string;
  title: string;
  version: string;
  base_url: string;
  deployment: {
    instance: string;
    region: string;
  };
}

interface ApiConfiguration {
  connectionType: ConnectionTypes;
  urls: {
    http: string;
    worker: string;
    websocket: string;
  };
}

export interface ServicesConfiguration {
  pollInterval: number;
  thresholds: {
    slow: number;
  };
}

export interface Configuration {
  app: BaseConfiguration;
  api: ApiConfiguration;
  services: ServicesConfiguration;
}
