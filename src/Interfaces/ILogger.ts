import winston from "winston";

export interface ILoggerConstructorParams {
    caller?: string;
    level?: string;
    formatParam?: winston.Logform.Format;
    logsFolder?: string;
    transports?: winston.transport[];
    exitOnError?: boolean;
    format?: winston.Logform.Format;
    silent?: boolean;
    id?: string;
    subCaller?: string;
}

abstract class ILogger {
    abstract setSubCaller(subCaller: string): ILogger;
    abstract info(message: string): ILogger;
    abstract error(message: string): ILogger;
    abstract debug(message: string): ILogger;
    abstract warn(message: string): ILogger;
}

export default ILogger;