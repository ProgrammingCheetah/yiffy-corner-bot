import * as Sequelize from 'sequelize';
import { DataTypes, Model, Optional } from 'sequelize';

export interface adminAttributes {
  id?: number;
  username: string;
}

export type adminPk = "id";
export type adminId = admin[adminPk];
export type adminOptionalAttributes = "id";
export type adminCreationAttributes = Optional<adminAttributes, adminOptionalAttributes>;

export class admin extends Model<adminAttributes, adminCreationAttributes> implements adminAttributes {
  id?: number;
  username!: string;


  static initModel(sequelize: Sequelize.Sequelize): typeof admin {
    return admin.init({
    id: {
      autoIncrement: true,
      type: DataTypes.INTEGER,
      allowNull: true,
      primaryKey: true
    },
    username: {
      type: DataTypes.TEXT,
      allowNull: false
    }
  }, {
    sequelize,
    tableName: 'admin',
    timestamps: false
  });
  }
}
