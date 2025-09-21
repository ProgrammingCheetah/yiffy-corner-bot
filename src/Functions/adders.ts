import { Sequelize } from "sequelize";
import { IGenerics } from '../Interfaces';
import { implementedOperations } from "../Interfaces/IGeneric";
import { admin, adminCreationAttributes } from '../models/admin';
import { tag, tagCreationAttributes } from '../models/tag';
function getSupportedModel(type: IGenerics.implementedOperations, db: Sequelize) {
    switch (type) {
        case 'admin':
            return db.models.admin;
        case 'defaultTag':
            return db.models.tag;
        case 'forbiddenTag':
            return db.models.tag;
        default:
            return null;

    }
}

async function Add(db: Sequelize, type: string, args: string[]): Promise<boolean> {
    try {
        if (!args.length) throw { botMessage: 'No arguments!' };
        const model = getSupportedModel(type as IGenerics.implementedOperations, db);
        if (!model) return false;
        for (const arg of args) {
            const subType = type === 'defaultTag' ? 'D' : 'F';
            const creationAttributes: adminCreationAttributes | tagCreationAttributes =
                type === 'admin'
                    ? {
                        username: arg
                    } : {
                        name: arg,
                        type: subType
                    };
            model.create(creationAttributes);
        }
        return true;
    } catch (error) {
        return false;
    }
}

async function Remove(db: Sequelize, type: string, args: string[]): Promise<boolean> {
    try {
        if (!args.length) throw { botMessage: 'No arguments!' };
        const model = getSupportedModel(type as IGenerics.implementedOperations, db);
        if (!model) return false;
        for (const arg of args) {
            const destructionObject = type === 'admin' ?
                { username: arg } : { name: arg };
            model.destroy({ where: destructionObject });
        }
        return true;
    } catch (error) {
        return false;
    }

}
async function List(db: Sequelize, type: string): Promise<string[]> {
    try {
        const model = getSupportedModel(type as IGenerics.implementedOperations, db);
        if (!model) return [];

        const identifier = type === 'admin' ? 'username' : 'name';
        const allResults = await model.findAll({ order: [[identifier, 'ASC']] });
        if (type === 'admin') {
            return (allResults as admin[]).map((adminModel: admin) => adminModel.username)
                // sort alphabetically
                .sort((a, b) => a[0].localeCompare(b[0]));
        }
        else {
            if (type === 'defaultTag') {
                return (allResults as tag[]).filter((tagModel: tag) => tagModel.type === 'D').map((tagModel: tag) => tagModel.name);
            }
            else {
                return (allResults as tag[]).filter((tagModel: tag) => tagModel.type === 'F').map((tagModel: tag) => tagModel.name);
            }

        }
    } catch (error) {
        return [];
    }
}

export default { Add, Remove, List };