import { IConfiguration, IFileSystem, ILogger } from '../Interfaces';

interface IInitConfig {
    token?: string;
    ownerId?: number;
    commands?: { [key: string]: any; };
    name?: string;
    cookies?: {
        a: string;
        b: string;
    };
}

interface IInitConfigOptions {
    fatal?: boolean;
    logger?: ILogger;
}

function GetInitConfiguration(cf: IConfiguration, fs: IFileSystem, options?: IInitConfigOptions): IInitConfig {
    const missing: string[] = [];

    const tokenDir = cf.getConfiguration<string>('bot.token', { fatal: true })!;
    const token = fs.readVaultFileSync(tokenDir);
    if (!token) {
        missing.push('Bot Token');
    }

    const ownerId = cf.getConfiguration<number>('owner.id', { fatal: true })!;
    if (!ownerId) {
        missing.push('Owner ID');
    }

    const commands = cf.getConfiguration<{ [key: string]: any; }>('commands');
    if (!commands) {
        missing.push('Commands');
    }

    const nameDir = cf.getConfiguration<string>('name', { fatal: true })!;
    const name = fs.readVaultFileSync(nameDir);
    if (!name) {
        missing.push('Name');
    }

    const cookies = cf.getConfiguration<{ a: string, b: string; }>('cookies');
    if (!cookies) {
        missing.push('Cookies');
    }

    if (missing.length) {
        if (options?.fatal) {
            throw new Error(`Missing configuration(s): ${missing.join(', ')}`);
        }
        options?.logger?.warn(`Missing configuration(s): ${missing.join(', ')}`);
    }
    return { token, ownerId, commands, name, cookies };
}

export default { GetInitConfiguration };