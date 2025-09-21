import ILogger from "./ILogger";

export interface IPostContainTagsOptions {
    fatal: boolean;
}

export interface IGetComponent {
    logger: ILogger;
}