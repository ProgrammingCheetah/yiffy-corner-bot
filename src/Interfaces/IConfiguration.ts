import c from "config";

export interface IConfOptions {
    prefix?: string;
}

interface IDefaultOptions {
    fatal?: boolean;
}


abstract class IConfiguration {
    abstract config(): c.IConfig;
    abstract getConfigurationArray<T>(options?: IConfOptions, ...configurations: string[]): (T | undefined)[];
    abstract getConfiguration<T>(name: string, options?: IDefaultOptions): T | undefined;
    abstract checkIfexists(name: string): boolean;
}



export default IConfiguration;