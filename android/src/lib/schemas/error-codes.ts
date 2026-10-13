import codes from "./error-codes.json";

export const ERROR_CODES = codes;

export type ErrorCode = (typeof ERROR_CODES)[number];
