import { IConfiguration, IFileSystem, ILogger, IOptions } from '../Interfaces';
import { Options, Sequelize } from 'sequelize';
import { initModels } from '../models/init-models';
import path from 'path';


function GetComponent(cf: IConfiguration, fs: IFileSystem, options: IOptions.IGetComponent): Sequelize {
    const dbSubpath: string = cf.getConfiguration<string>('db', { fatal: true })!;
    const dbPath = path.join(fs.getVaultPath(), dbSubpath);
    const sequelizeOptions: Options = {
        dialect: 'sqlite',
        storage: dbPath,
        logging: options?.logger.info,
    };

    const sequelize = new Sequelize(sequelizeOptions);
    initModels(sequelize);
    return sequelize;
}


export default { GetComponent };