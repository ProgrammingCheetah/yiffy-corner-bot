import * as Sequelize from 'sequelize';
import { DataTypes, Model, Optional } from 'sequelize';

export interface tagAttributes {
  id?: number;
  name: string;
  type: string;
}

export type tagPk = "id";
export type tagId = tag[tagPk];
export type tagOptionalAttributes = "id" | "type";
export type tagCreationAttributes = Optional<tagAttributes, tagOptionalAttributes>;

export class tag extends Model<tagAttributes, tagCreationAttributes> implements tagAttributes {
  id?: number;
  name!: string;
  type!: string;


  static initModel(sequelize: Sequelize.Sequelize): typeof tag {
    return tag.init({
    id: {
      autoIncrement: true,
      type: DataTypes.INTEGER,
      allowNull: true,
      primaryKey: true
    },
    name: {
      type: DataTypes.TEXT,
      allowNull: false
    },
    type: {
      type: DataTypes.TEXT,
      allowNull: false,
      defaultValue: "F"
    }
  }, {
    sequelize,
    tableName: 'tag',
    timestamps: false
  });
  }
}
