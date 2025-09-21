import config from 'config';
import IConfiguration, { IConfOptions } from '../Interfaces/IConfiguration';
type ConfigurationImplementedTypes = IConfiguration;

class CConfiguration implements IConfiguration {
    config = () => config;
    checkIfexists(name: string): boolean {
        return config.has(name);
    }

    getConfiguration<T>(name: string, options?: { fatal?: boolean; }): T | undefined {
        const exists = this.checkIfexists(name);
        if (!exists) {
            if (options?.fatal) {
                throw new Error(`Configuration ${name} does not exist`);
            }
            return undefined;
        }
        return config.get(name) as T;

    }

    getConfigurationArray<T>(options?: IConfOptions, ...configurations: string[]): T[] {
        const prefix = options?.prefix || '';
        const local: string[] = prefix
            ? configurations.map((configuration) => `${prefix}.${configuration}`)
            : [...configurations];
        return local.map((configuration => config.get(configuration)));
    }
}

function GetComponent(): ConfigurationImplementedTypes {
    return new CConfiguration();
}

export default { GetComponent };
