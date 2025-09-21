import { Sequelize } from 'sequelize';
import { admin } from "../models/admin";

async function AdminCheckpoint(db: Sequelize, username: string): Promise<void> {
    const adminModel: admin = await db.models.admin.findOne({ where: { username } }) as admin;
    if (!adminModel) throw { botMessage: 'Forbidden!' };
}

async function OwnerCheckpoint(ownerId: number, messageOwnerId?: number,) {
    if (messageOwnerId !== ownerId) throw { botMessage: 'Only the owner can do this!' };
}


export default { AdminCheckpoint, OwnerCheckpoint };