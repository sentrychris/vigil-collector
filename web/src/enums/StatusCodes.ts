export enum HttpStatus {
  Unauthorized = 401,
  Forbidden = 403,
  NotFound = 404,
  TooManyRequests = 429,
  InternalServerError = 500,
}

export enum WebsocketStatus {
  ClosedNormal = 1000,
  ClosedProtocolError = 1002,
  ClosedNoStatus = 1005,
  InternalServerError = 1011,
}
