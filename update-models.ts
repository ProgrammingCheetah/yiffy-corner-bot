import { SequelizeAuto } from 'sequelize-auto';
import { Sequelize } from 'sequelize';

const sequelize = new Sequelize({
    dialect: 'sqlite',
    storage: './config/vault/storage/db.sqlite'
});

// @ts-ignore
const auto = new SequelizeAuto(sequelize, null, null, {
    dialect: "sqlite",
    directory: "./src/models",
    lang: 'ts',
    additional: {
        timestamps: false
    }
});


auto.run().then();