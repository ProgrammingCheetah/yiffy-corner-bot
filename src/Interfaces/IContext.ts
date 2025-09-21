import { Context } from "telegraf";
import ILogger from "./ILogger";
export interface IContextExtended extends Context {
    logger: ILogger;
    uuid: string;
    username: string;
}

export interface IDefaultOptions {
    logger?: ILogger;
    id?: string;
    username?: string;
}

export interface IRunFetchOptions extends IDefaultOptions {
    defaultTagsOverride?: string[];
    forbiddenTagsOverride?: string[];
}

export interface ISendPostOptions {
    force?: boolean;
}

export interface IChangelog {
    [key: string]: {
        Added: string[];
        Removed: string[];
        Fixed: string[];
    };
}