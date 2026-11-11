import type { App } from "vue";
import axios, { type AxiosInstance, type AxiosRequestConfig, type AxiosResponse } from "axios";
import { config } from "@/config";
import { injectHttp } from "@/injection";
import { HttpStatus } from "@/enums/StatusCodes";

export class HttpMaker {
  private instance: AxiosInstance | null = null;

  private get http(): AxiosInstance {
    return this.instance != null ? this.instance : this.init();
  }

  init() {
    const http = axios.create({
      baseURL: config.api.urls.http,
      headers: {
        accept: "application/json",
        "content-type": "application/json; charset=utf-8",
      },
    });

    http.interceptors.request.use(
      (config) => {
        try {
          const token = localStorage.getItem("accessToken");

          if (token != null && config.headers) {
            config.headers.Authorization = `Bearer ${token}`;
          }

          return config;
        } catch (error) {
          if (error instanceof Error) {
            throw new Error(error.message);  // Error object caught, re-throwing
          } else {
            throw new Error("Unknown error occurred while adding Authorization header");
          }
        }
      },
      (error) => Promise.reject(error),
    );

    http.interceptors.response.use(
      (response) => response,
      (error) => {
        const { response } = error;
        return this.handleError(response);
      },
    );

    this.instance = http;

    return http;
  }

  async request<T, R = AxiosResponse<T>>(config: AxiosRequestConfig): Promise<R> {
    return this.http.request(config);
  }

  async get<T, R = AxiosResponse<T>>(url: string, config?: AxiosRequestConfig): Promise<R> {
    return this.http.get<T, R>(url, config);
  }

  async post<T, R = AxiosResponse<T>>(
    url: string,
    data?: T,
    config?: AxiosRequestConfig,
  ): Promise<R> {
    return this.http.post<T, R>(url, data, config);
  }

  async put<T, R = AxiosResponse<T>>(
    url: string,
    data?: T,
    config?: AxiosRequestConfig,
  ): Promise<R> {
    return this.http.put<T, R>(url, data, config);
  }

  async delete<T, R = AxiosResponse<T>>(
    url: string,
    config?: AxiosRequestConfig,
  ): Promise<R> {
    return this.http.delete<T, R>(url, config);
  }

  private handleError(error: Response) {
    const { status } = error;

    switch (status) {
      case HttpStatus.InternalServerError: {
        // Handle InternalServerError
        break;
      }
      case HttpStatus.Forbidden: {
        // Handle Forbidden
        break;
      }
      case HttpStatus.Unauthorized: {
        // Handle Unauthorized
        break;
      }
      case HttpStatus.TooManyRequests: {
        // Handle TooManyRequests
        break;
      }
    }

    return Promise.reject(error);
  }
}

export const http = new HttpMaker();

export const useHttp = {
  install(app: App) {
    app.provide(injectHttp, http);
  },
};
